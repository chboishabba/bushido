import { useState, useEffect, useRef, useCallback, memo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

let nextFindRequest = 0;
interface Props {
  tabId: string;
  onClose: () => void;
}

export default memo(function FindBar({ tabId, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [matchCount, setMatchCount] = useState<number | null>(null);
  const [activeIdx, setActiveIdx] = useState(0);
  const [nativeCount, setNativeCount] = useState(false);
  const [pending, setPending] = useState(false);
  const [findError, setFindError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const requestRef = useRef(0);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    let alive = true;
    const nav = listen<{ id: string; loading: boolean }>("tab-loading", (e) => {
      if (alive && e.payload.id === tabId && e.payload.loading) {
        requestRef.current = 0;
        setMatchCount(null);
        setActiveIdx(0);
        setPending(false);
        setFindError(null);
      }
    });
    const p = listen<{ id: string; count: number; requestId?: number }>("match-count", (e) => {
      if (alive && requestRef.current !== 0 && e.payload.id === tabId && e.payload.requestId === requestRef.current) {
        setNativeCount(false);
        setMatchCount(e.payload.count);
      }
    });
    return () => {
      alive = false;
      requestRef.current = 0;
      p.then(u => u());
      nav.then(u => u());
      invoke("find_in_page", { id: tabId, query: "", forward: true, requestId: ++nextFindRequest })
        .catch(error => console.warn("[bushido] clear find", error));
    };
  }, [tabId]);

  const find = useCallback((forward = true) => {
    if (pending || !query) return;
    const requestId = ++nextFindRequest;
    requestRef.current = requestId;
    setPending(true);
    setFindError(null);
    invoke<number | null>("find_in_page", { id: tabId, query, forward, requestId })
      .then(count => {
        if (requestRef.current === requestId && count !== null) {
          setNativeCount(true);
          setMatchCount(count);
        }
      })
      .catch(error => {
        if (requestRef.current === requestId) {
          setMatchCount(null);
          setFindError(String(error));
        }
        console.warn("[bushido] find-in-page", error);
      }).finally(() => {
        if (requestRef.current === requestId) setPending(false);
      });
    if (matchCount && matchCount > 0) {
      setActiveIdx(prev => {
        if (forward) return prev >= matchCount ? 1 : prev + 1;
        return prev <= 1 ? matchCount : prev - 1;
      });
    }
  }, [tabId, query, matchCount, pending]);

  const close = useCallback(() => {
    requestRef.current = 0;
    // Effect cleanup clears native highlights even on external close.
    setMatchCount(null);
    setActiveIdx(0);
    onClose();
  }, [onClose]);

  // reset index when query changes
  useEffect(() => {
    requestRef.current = 0;
    setMatchCount(null);
    setActiveIdx(query ? 1 : 0);
    setPending(false);
    setFindError(null);
  }, [query, tabId]);

  const countLabel = matchCount !== null
    ? (matchCount === 4294967295 && nativeCount ? "100,000+ matches" :
      matchCount > 0 ? (nativeCount ? `${matchCount} matches` : `${Math.min(activeIdx, matchCount)} of ${matchCount}`) : "no matches")
    : null;

  return (
    <div className="find-bar">
      <input
        ref={inputRef}
        className="find-input"
        value={query}
        onChange={e => {
          requestRef.current = 0;
          setQuery(e.target.value);
          invoke("find_in_page", { id: tabId, query: "", forward: true, requestId: ++nextFindRequest })
            .catch(error => console.warn("[bushido] clear find", error));
        }}
        onKeyDown={e => {
          if (e.key === "Enter") find(!e.shiftKey);
          if (e.key === "Escape") close();
        }}
        placeholder="find in page..."
        spellCheck={false}
      />
      {findError && <span className="find-count find-count-none" title={findError}>find unavailable</span>}
      {countLabel !== null && (
        <span className={`find-count ${matchCount === 0 ? "find-count-none" : "find-count-active"}`}>{countLabel}</span>
      )}
      <button className="find-btn" onClick={() => find(false)} disabled={pending || !query} title="Previous">
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M2 8L6 4L10 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      </button>
      <button className="find-btn" onClick={() => find(true)} disabled={pending || !query} title="Next">
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M2 4L6 8L10 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      </button>
      <button className="find-btn" onClick={close} title="Close (Esc)">
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M2 2L10 10M10 2L2 10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
        </svg>
      </button>
    </div>
  );
});
