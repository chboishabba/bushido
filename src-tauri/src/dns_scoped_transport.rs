//! Internal one-shot transport receiver; not a listening proxy or public API.
//! The only runnable destination constructor is test-only loopback. A real
//! trusted scope producer must be reviewed before a production caller exists.
#![allow(dead_code)]
use super::{AddressCandidate, Policy, Resolver, ResolverControl};
use std::{future::{poll_fn, Future}, net::{IpAddr, SocketAddr}, sync::Arc,
          task::Poll, time::{Duration, Instant}};
use tokio::{io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
            net::TcpStream, sync::watch};

const MAX_SECONDS: u64 = 12;
const MAX_BYTES_EACH: u64 = 2 * 1024 * 1024;

// Private fields, no deserialization, no browser-controlled constructor.
// An approved address is not derived from the DNS answer itself.
struct ApprovedScope {
    hostname: String,
    address: SocketAddr,
}

impl ApprovedScope {
    #[cfg(test)]
    fn loopback_fixture(hostname: &str, address: SocketAddr) -> Self {
        assert!(address.ip().is_loopback() && address.port() >= 1024);
        Self { hostname: hostname.into(), address }
    }
}

struct Admitted<'a> {
    owner: &'a Resolver,
    policy: Policy,
    address: SocketAddr,
    deadline: Instant,
    events: watch::Receiver<u64>,
    _slot: tokio::sync::SemaphorePermit<'a>,
}

fn admit<'a>(control: &'a ResolverControl, candidate: AddressCandidate,
             scope: ApprovedScope, now: Instant) -> Result<Admitted<'a>, String> {
    let r = &control.0;
    if !candidate.owner.upgrade().is_some_and(|owner| Arc::ptr_eq(&owner,r)) {
        return Err("Wrong resolver instance".into());
    }
    if candidate.hostname != scope.hostname || !candidate.addresses.contains(&scope.address.ip())
        || now >= candidate.expires {
        return Err("Destination outside exact scope or expired".into());
    }
    // Only the test build can connect to loopback. All non-test callers
    // would need a new reviewed constructor; fail closed even if one is added.
    #[cfg(not(test))]
    return Err("Live trusted destination scope producer not implemented".into());

    #[cfg(test)]
    {
        if !scope.address.ip().is_loopback() { return Err("Fixture not loopback".into()); }
        let policy = r.policy.read();
        if *policy != candidate.policy { return Err("Stale policy".into()); }
        let slot = r.transport_slots.try_acquire().map_err(|_| "Transport busy")?;
        let events = r.policy_events.subscribe();
        let deadline = candidate.expires.min(now.checked_add(Duration::from_secs(MAX_SECONDS))
            .ok_or("Invalid lease deadline")?);
        Ok(Admitted { owner:r.as_ref(), policy:*policy, address:scope.address, deadline,
                      events, _slot:slot })
    }
}

impl Admitted<'_> {
    // Every future poll (including initial connect and I/O wakeups) is
    // ordered against policy writes. No synchronous lock spans an await.
    async fn guarded<F: Future>(&self, future: F) -> Result<F::Output, String> {
        let mut future = std::pin::pin!(future);
        let mut changes = self.events.clone();
        let polled = poll_fn(|cx| {
            let policy = self.owner.policy.read();
            if *policy != self.policy || Instant::now() >= self.deadline {
                return Poll::Ready(Err("Transport lease revoked or expired".into()));
            }
            future.as_mut().poll(cx).map(Ok)
        });
        tokio::select! {
            biased;
            _ = changes.changed() => Err("Transport policy changed".into()),
            result = tokio::time::timeout_at(tokio::time::Instant::from_std(self.deadline), polled) =>
                result.map_err(|_| "Transport lease expired".to_string())?,
        }
    }

    async fn connect(&self) -> Result<TcpStream, String> {
        let stream = self.guarded(TcpStream::connect(self.address)).await?
            .map_err(|_| "Scoped TCP connection failed")?;
        if stream.peer_addr().ok() != Some(self.address) {
            return Err("TCP peer mismatch".into());
        }
        self.guarded(std::future::ready(())).await?;
        Ok(stream)
    }

    async fn pipe<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &self, mut read: R, mut write: W,
    ) -> Result<u64, String> {
        let mut sent = 0u64;
        let mut buffer = [0u8; 8192];
        loop {
            // Read no more than one excess byte. Never forward that byte.
            let limit = (MAX_BYTES_EACH - sent + 1).min(buffer.len() as u64) as usize;
            let n = self.guarded(read.read(&mut buffer[..limit])).await?
                .map_err(|_| "Scoped relay read failed")?;
            if n == 0 {
                self.guarded(write.shutdown()).await?.map_err(|_| "Scoped half-close failed")?;
                return Ok(sent);
            }
            if n as u64 > MAX_BYTES_EACH - sent {
                return Err("Scoped relay byte limit".into());
            }
            self.guarded(write.write_all(&buffer[..n])).await?
                .map_err(|_| "Scoped relay write failed")?;
            sent += n as u64;
        }
    }

    // Consume the admitted destination and downstream socket. No raw
    // upstream socket returned to a caller who could bypass its lease.
    async fn relay(self, downstream: TcpStream) -> Result<(u64,u64), String> {
        let upstream = self.connect().await?;
        let (dr,dw) = downstream.into_split();
        let (ur,uw) = upstream.into_split();
        tokio::try_join!(self.pipe(dr,uw),self.pipe(ur,dw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns_resolver::PrivacyLevel;
    use tokio::net::TcpListener;

    fn fixture_wire(ip: IpAddr) -> (Vec<u8>,Vec<u8>) {
        let IpAddr::V4(ip) = ip else { panic!("IPv4 fixture only") };
        let mut query = vec![0,42,1,0,0,1,0,0,0,0,0,0,7];
        query.extend_from_slice(b"fixture");
        query.extend_from_slice(&[4]);
        query.extend_from_slice(b"test");
        query.extend_from_slice(&[0,0,1,0,1]);
        let mut response = query.clone();
        response[2]=0x81; response[3]=0x80; response[7]=1;
        response.extend_from_slice(&[0xc0,0x0c,0,1,0,1,0,0,0,3,0,4]);
        response.extend_from_slice(&ip.octets());
        (query,response)
    }

    fn candidate(control: &ResolverControl, ip: IpAddr, now: Instant) -> AddressCandidate {
        let (query,response)=fixture_wire(ip);
        let snapshot=*control.0.policy.read();
        control.candidate_from_wire(&query,&response,snapshot,now,now).unwrap()
    }

    #[test]
    fn original_wire_constructor_rejects_mismatched_response() {
        let control=control();
        let now=Instant::now();
        let (query,mut response)=fixture_wire("127.0.0.1".parse().unwrap());
        let snapshot=*control.0.policy.read();
        response[1]^=1;
        assert!(control.candidate_from_wire(&query,&response,snapshot,now,now).is_err());
        response[1]^=1;
        assert!(control.candidate_from_wire(&query,&response,snapshot,now,now+Duration::from_secs(3)).is_err());
        control.set_level("maximum").unwrap();
        assert!(control.candidate_from_wire(&query,&response,snapshot,now,now).is_err());
    }

    fn control() -> ResolverControl {
        ResolverControl(Resolver::new(PrivacyLevel::Strict).unwrap())
    }

    #[tokio::test(flavor="current_thread")]
    async fn exact_peer_and_opaque_relay() {
        let control=control();
        let origin=TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=origin.local_addr().unwrap();
        let downstream=TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut peer=TcpStream::connect(downstream.local_addr().unwrap()).await.unwrap();
        let (client,_)=downstream.accept().await.unwrap();
        let now=Instant::now();
        let lease=admit(&control,candidate(&control,address.ip(),now),
                       ApprovedScope::loopback_fixture("fixture.test.",address),now).unwrap();
        let server=async {
            let (mut stream,_)=origin.accept().await.unwrap();
            let mut data=[0;4];
            stream.read_exact(&mut data).await.unwrap();
            assert_eq!(&data,b"ping");
            stream.write_all(b"pong").await.unwrap();
            stream.shutdown().await.unwrap();
            let mut end=[0;1];
            assert_eq!(stream.read(&mut end).await.unwrap(),0);
        };
        let browser=async {
            peer.write_all(b"ping").await.unwrap();
            peer.shutdown().await.unwrap();
            let mut data=[0;4];
            peer.read_exact(&mut data).await.unwrap();
            assert_eq!(&data,b"pong");
        };
        let (receipt,_,_)=tokio::time::timeout(Duration::from_secs(5),async {
            tokio::join!(lease.relay(client),server,browser)
        }).await.expect("bounded loopback fixture");
        assert_eq!(receipt.unwrap(),(4,4));
    }

    #[tokio::test(flavor="current_thread")]
    async fn policy_change_cancels_pending_operation() {
        let control=control();
        let now=Instant::now();
        let address:SocketAddr="127.0.0.1:23456".parse().unwrap();
        let lease=admit(&control,candidate(&control,address.ip(),now),
                       ApprovedScope::loopback_fixture("fixture.test.",address),now).unwrap();
        let revocation=async {
            tokio::task::yield_now().await;
            control.set_level("maximum").unwrap();
            control.set_level("strict").unwrap();
        };
        let (result,_)=tokio::time::timeout(Duration::from_secs(1),async {
            tokio::join!(lease.guarded(std::future::pending::<()>()),revocation)
        }).await.expect("policy notification must wake pending operation before TTL");
        assert!(result.is_err());
        assert!(lease.connect().await.is_err());
    }

    #[test]
    fn scope_mismatch_and_slots() {
        let c=control();
        let now=Instant::now();
        let address:SocketAddr="127.0.0.1:23456".parse().unwrap();
        let scope=|| ApprovedScope::loopback_fixture("fixture.test.",address);
        assert!(admit(&c,candidate(&c,"127.0.0.2".parse().unwrap(),now),scope(),now).is_err());
        let mut leases=Vec::new();
        for _ in 0..8 { leases.push(admit(&c,candidate(&c,address.ip(),now),scope(),now).unwrap()); }
        assert!(admit(&c,candidate(&c,address.ip(),now),scope(),now).is_err());
        leases.pop();
        assert!(admit(&c,candidate(&c,address.ip(),now),scope(),now).is_ok());
    }
}
