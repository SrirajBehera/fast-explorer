
interface StatusBarProps {
  total: number;
  query: string;
  resultCount: number;
  isSearching: boolean;
  searchTime?: number | null;
}

export function StatusBar({ total, query, resultCount, isSearching, searchTime }: StatusBarProps) {
  return (
    <div className="flex items-center justify-between px-4 py-2 border-t border-white/[0.06] shrink-0 select-none">
      <div className="flex items-center gap-3 text-[11px] text-white/25 font-mono">
        {query ? (
          <div className="flex items-center gap-3">
            {isSearching ? (
              <span className="text-indigo-400/60 animate-pulse">Searching…</span>
            ) : (
              <span>
                <span className="text-white/40">{resultCount}</span> results
              </span>
            )}
            {searchTime !== undefined && searchTime !== null && !isSearching && (
              <span className="flex items-center gap-0.5 text-indigo-400/80 font-bold bg-indigo-500/10 px-1.5 py-0.5 rounded-md border border-indigo-500/15">
                <span>⚡</span>
                <span>{searchTime.toFixed(2)}ms</span>
              </span>
            )}
          </div>
        ) : (
          <span>{total.toLocaleString()} files indexed</span>
        )}
      </div>
      <div className="flex items-center gap-2 text-[10px] text-white/20">
        <span className="flex items-center gap-1">
          <kbd className="bg-white/5 px-1.5 py-0.5 rounded font-mono">↑↓</kbd>
          navigate
        </span>
        <span className="flex items-center gap-1">
          <kbd className="bg-white/5 px-1.5 py-0.5 rounded font-mono">↵</kbd>
          open
        </span>
        <span className="flex items-center gap-1">
          <kbd className="bg-white/5 px-1.5 py-0.5 rounded font-mono">⌘C</kbd>
          copy path
        </span>
      </div>
    </div>
  );
}
