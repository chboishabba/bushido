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
  const inputRef = useRef<HTMLInputElement>(null);
  const requestRef = useRef(0);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const p = listen<{ id: string; count: number; requestId?: number }>("match-count", (e) => {
      if (e.payload.id === tabId && e.payload.requestId === requestRef.current) {
        setMatchCount(e.payload.count);
      }
    });
    return () => { requestRef.current = 0; p.then(u => u()); };
  }, [tabId]);

  const find = useCallback((forward = true) => {
    const requestId = ++nextFindRequest;
    requestRef.current = requestId;
    invoke<number | null>("find_in_page", { id: tabId, query, forward, requestId })
      .then(count => {
        if (requestRef.current === requestId && count !== null) setMatchCount(count);
      })
      .catch(error => {
        if (requestRef.current === requestId) setMatchCount(null);
        console.warn("[bushido] find-in-page", error);
      });
    if (matchCount && matchCount > 0) {
      setActiveIdx(prev => {
        if (forward) return prev >= matchCount ? 1 : prev + 1;
        return prev <= 1 ? matchCount : prev - 1;
      });
    }
  }, [tabId, query, matchCount]);

  const close = useCallback(() => {
    requestRef.current = 0;
    invoke("find_in_page", { id: tabId, query: "", forward: true, requestId: ++nextFindRequest })
      .catch(error => console.warn("[bushido] clear find", error));
    setMatchCount(null);
    setActiveIdx(0);
    onClose();
  }, [tabId, onClose]);

  // reset index when query changes
  useEffect(() => {
    requestRef.current = 0;
    setMatchCount(null);
    setActiveIdx(query ? 1 : 0);
  }, [query, tabId]);

  const countLabel = matchCount !== null
    ? (matchCount > 0 ? `${Math.min(activeIdx, matchCount)} of ${matchCount}` : "no matches")
    : null;

  return (
    <div className="find-bar">
      <input
        ref={inputRef}
        className="find-input"
        value={query}
        onChange={e => { requestRef.current = 0; setQuery(e.target.value); }}
        onKeyDown={e => {
          if (e.key === "Enter") find(!e.shiftKey);
          if (e.key === "Escape") close();
        }}
        placeholder="find in page..."
        spellCheck={false}
      />
      {countLabel !== null && (
        <span className={`find-count ${matchCount === 0 ? "find-count-none" : "find-count-active"}`}>{countLabel}</span>
      )}
      <button className="find-btn" onClick={() => find(false)} title="Previous">
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M2 8L6 4L10 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      </button>
      <button className="find-btn" onClick={() => find(true)} title="Next">
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
