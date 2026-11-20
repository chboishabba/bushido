//! Narrow replay constructor for synthetic/debug resolver acceptance.
//! The existing Hickory request/response validation must precede insertion.
//! No EDNS, DNSSEC, aliases, negative answers, authority or additional records.
use std::{collections::HashMap, time::{Duration, Instant}};

const CAPACITY: usize = 256;
const MAX_PACKET: usize = 4096;
const MAX_LIFETIME: u32 = 60;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct Key { generation: u64, query_without_id: Vec<u8> }

fn u16_at(wire: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(wire.get(i..i+2)?.try_into().ok()?))
}

fn u32_at(wire: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(wire.get(i..i+4)?.try_into().ok()?))
}

fn query_end(wire: &[u8]) -> Option<(usize, u16)> {
    if wire.len() < 17 || wire.len() > 512 || wire[2..12] != [1,0,0,1,0,0,0,0,0,0] {
        return None;
    }
    let mut cursor = 12;
    loop {
        let n = *wire.get(cursor)? as usize;
        cursor += 1;
        if n == 0 { break; }
        if n > 63 || cursor + n > 267 { return None; }
        let label = wire.get(cursor..cursor+n)?;
        if !label.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-') { return None; }
        cursor += n;
    }
    if cursor == 13 || cursor + 4 != wire.len() { return None; }
    let kind = u16_at(wire, cursor)?;
    if !matches!(kind, 1 | 28) || u16_at(wire, cursor+2)? != 1 { return None; }
    Some((cursor, kind))
}

pub(crate) fn key(wire: &[u8], generation: u64) -> Option<Key> {
    query_end(wire)?;
    Some(Key { generation, query_without_id: wire[2..].to_vec() })
}

struct Entry {
    packet: Vec<u8>,
    ttl_offsets: Vec<(usize, u32)>,
    started: Instant,
    lifetime: u32,
}

// Only accept explicit simple HTTP max-age directive. Anything ambiguous
// (including no-store, no-cache, private, Vary) bypasses the DNS cache.
pub(crate) fn explicit_max_age(value: &str) -> Option<u32> {
    let mut age = None;
    for directive in value.split(',').map(str::trim) {
        if directive.eq_ignore_ascii_case("public") { continue; }
        let (name, value) = directive.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("max-age") || age.is_some() { return None; }
        let value = value.trim();
        if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) { return None; }
        age = Some(value.parse().ok()?);
    }
    age
}

fn make_entry(query: &[u8], packet: &[u8], started: Instant, max_age: u32) -> Option<Entry> {
    let (name_end, kind) = query_end(query)?;
    // Exact original transaction and question bytes; flags QR/RD/RA only.
    if packet.len() > MAX_PACKET || packet.len() < query.len()
        || packet[0..2] != query[0..2] || packet[2] != 0x81
        || !matches!(packet[3], 0 | 0x80)
        || u16_at(packet, 4)? != 1 || u16_at(packet, 8)? != 0 || u16_at(packet, 10)? != 0
        || packet[12..query.len()] != query[12..] {
        return None;
    }
    let count = u16_at(packet, 6)? as usize;
    if count == 0 || count > 64 { return None; }
    let mut cursor = query.len();
    let mut offsets = Vec::with_capacity(count);
    let mut lifetime = max_age.min(MAX_LIFETIME);
    for _ in 0..count {
        // Only accept direct owner of the question. No arbitrary compression
        // chains: either pointer to original QNAME or exact literal QNAME.
        if packet.get(cursor..cursor+2)? == [0xc0,0x0c] {
            cursor += 2;
        } else {
            let len = name_end - 12;
            if packet.get(cursor..cursor+len)? != &query[12..name_end] { return None; }
            cursor += len;
        }
        if u16_at(packet,cursor)? != kind || u16_at(packet,cursor+2)? != 1 { return None; }
        let ttl = u32_at(packet,cursor+4)?;
        let rdlen = u16_at(packet,cursor+8)? as usize;
        if rdlen != if kind == 1 { 4 } else { 16 } { return None; }
        packet.get(cursor+10..cursor+10+rdlen)?;
        offsets.push((cursor+4,ttl));
        lifetime = lifetime.min(ttl);
        cursor += 10+rdlen;
    }
    if cursor != packet.len() || lifetime == 0 { return None; }
    Some(Entry { packet: packet.to_vec(), ttl_offsets: offsets, started, lifetime })
}

fn ceil_seconds(duration: Duration) -> u64 {
    duration.as_secs().saturating_add(u64::from(duration.subsec_nanos() != 0))
}

#[derive(Default)]
pub(crate) struct PositiveCache { entries: HashMap<Key, Entry> }

impl PositiveCache {
    pub(crate) fn clear(&mut self) { self.entries.clear(); }
    pub(crate) fn len(&self) -> usize { self.entries.len() }

    pub(crate) fn insert(&mut self, key: Key, query: &[u8], packet: &[u8],
                         started: Instant, now: Instant, max_age: u32) {
        if self::key(query, key.generation).as_ref() != Some(&key) { return; }
        let Some(entry) = make_entry(query, packet, started, max_age) else { return; };
        if ceil_seconds(now.saturating_duration_since(started)) >= u64::from(entry.lifetime) { return; }
        if self.entries.len() >= CAPACITY && !self.entries.contains_key(&key) {
            self.entries.clear(); // deterministic bounded storage; no unbounded LRU
        }
        self.entries.insert(key, entry);
    }

    pub(crate) fn lookup(&mut self, key: &Key, query: &[u8], now: Instant) -> Option<Vec<u8>> {
        if self::key(query, key.generation).as_ref() != Some(key) { return None; }
        let entry = self.entries.get(key)?;
        let elapsed = ceil_seconds(now.saturating_duration_since(entry.started));
        if elapsed >= u64::from(entry.lifetime) {
            self.entries.remove(key);
            return None;
        }
        let remaining = entry.lifetime - elapsed as u32;
        let mut packet = entry.packet.clone();
        packet[..2].copy_from_slice(query.get(..2)?);
        for &(offset, ttl) in &entry.ttl_offsets {
            let aged = (ttl - elapsed as u32).min(remaining);
            packet[offset..offset+4].copy_from_slice(&aged.to_be_bytes());
        }
        Some(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query() -> Vec<u8> {
        vec![0,1,1,0,0,1,0,0,0,0,0,0,1,b'a',4,b't',b'e',b's',b't',0,0,1,0,1]
    }
    fn response(q: &[u8], ttl: u32) -> Vec<u8> {
        let mut r = q.to_vec();
        r[2]=0x81; r[3]=0x80; r[7]=1;
        r.extend([0xc0,0x0c,0,1,0,1]);
        r.extend(ttl.to_be_bytes());
        r.extend([0,4,192,0,2,1]);
        r
    }
    #[test]
    fn replay_id_and_conservative_ttl() {
        let q=query(); let t=Instant::now(); let mut c=PositiveCache::default();
        let k=key(&q,0).unwrap();
        c.insert(k.clone(), &q, &response(&q,120), t,t,30);
        let mut q2=q.clone(); q2[1]=42;
        assert_eq!(key(&q2,0),Some(k.clone()));
        let r=c.lookup(&k,&q2,t+Duration::from_millis(1200)).unwrap();
        assert_eq!(&r[..2],&q2[..2]);
        assert_eq!(u32_at(&r,q.len()+6),Some(28));
        assert!(c.lookup(&k,&q2,t+Duration::from_secs(30)).is_none());
    }
    #[test]
    fn rejects_other_policy_question_and_envelopes() {
        let q=query(); let k=key(&q,1).unwrap();
        assert_ne!(key(&q,2),Some(k));
        let mut other=q.clone(); other[13]=b'b';
        assert_ne!(key(&q,1),key(&other,1));
        let mut edns=q.clone(); edns[11]=1;
        assert!(key(&edns,1).is_none());
        let mut r=response(&q,30); r[3]|=0x20;
        assert!(make_entry(&q,&r,Instant::now(),30).is_none());
        let mut r=response(&q,30); r[q.len()+3]=5;
        assert!(make_entry(&q,&r,Instant::now(),30).is_none());
        let mut r=response(&q,30); r.push(0);
        assert!(make_entry(&q,&r,Instant::now(),30).is_none());
        let mut r=response(&q,30); r[11]=1;
        assert!(make_entry(&q,&r,Instant::now(),30).is_none());
        assert!(make_entry(&q,&response(&q,0),Instant::now(),30).is_none());
    }
    #[test]
    fn http_freshness_narrowly_admitted() {
        assert_eq!(explicit_max_age("public, max-age=30"),Some(30));
        for s in ["no-store, max-age=30","max-age=1, max-age=2","private, max-age=1","max-age=-1",""] {
            assert_eq!(explicit_max_age(s),None);
        }
    }
    #[test]
    fn capacity_and_clear() {
        let mut c=PositiveCache::default(); let t=Instant::now(); let q=query();
        for generation in 0..=CAPACITY as u64 {
            c.insert(key(&q,generation).unwrap(),&q,&response(&q,30),t,t,30);
            assert!(c.len() <= CAPACITY);
        }
        c.clear(); assert_eq!(c.len(),0);
    }
}
