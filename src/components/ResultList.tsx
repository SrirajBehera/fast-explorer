import { useRef, useEffect } from "react";
import { FileIcon } from "./FileIcon";
import { clsx } from "clsx";

export interface SearchResult {
  name: string;
  path: string;
  is_dir: boolean;
}

interface ResultListProps {
  results: SearchResult[];
  selectedIndex: number;
  onSelect: (index: number) => void;
  onOpen: (result: SearchResult) => void;
  query: string;
  searchLimit: number;
  onLoadMore: () => void;
}

function HighlightedName({ name, query }: { name: string; query: string }) {
  if (!query) return <span>{name}</span>;
  const lowerName = name.toLowerCase();
  const lowerQuery = query.toLowerCase();
  const idx = lowerName.indexOf(lowerQuery);
  if (idx === -1) return <span>{name}</span>;
  return (
    <span>
      {name.slice(0, idx)}
      <span className="text-indigo-400 font-semibold">
        {name.slice(idx, idx + query.length)}
      </span>
      {name.slice(idx + query.length)}
    </span>
  );
}

interface ResultItemProps {
  result: SearchResult;
  index: number;
  isActive: boolean;
  onSelect: (index: number) => void;
  onOpen: (result: SearchResult) => void;
  query: string;
}

function ResultItem({ result, index, isActive, onSelect, onOpen, query }: ResultItemProps) {
  const itemRef = useRef<HTMLDivElement>(null);

  // Auto-scroll the active item into view
  useEffect(() => {
    if (isActive && itemRef.current) {
      itemRef.current.scrollIntoView({ block: "nearest" });
    }
  }, [isActive]);

  const pathParts = result.path.split("/");
  const displayPath =
    pathParts.length > 3
      ? "…/" + pathParts.slice(-3, -1).join("/")
      : pathParts.slice(0, -1).join("/");

  return (
    <div
      ref={itemRef}
      className={clsx("result-item", isActive && "active")}
      onClick={() => onSelect(index)}
      onDoubleClick={() => onOpen(result)}
    >
      <FileIcon name={result.name} isDir={result.is_dir} path={result.path} />

      <div className="flex flex-col min-w-0 flex-1">
        <span className="text-[14px] font-medium text-white/90 truncate leading-snug">
          <HighlightedName name={result.name} query={query} />
        </span>
        <span className="text-[11px] text-white/30 truncate mt-0.5 font-mono">
          {displayPath}
        </span>
      </div>

      {isActive && (
        <div className="flex items-center gap-1 shrink-0">
          <kbd className="text-[10px] text-white/30 bg-white/5 px-1.5 py-0.5 rounded font-mono">
            ↵
          </kbd>
        </div>
      )}
    </div>
  );
}

export function ResultList({
  results,
  selectedIndex,
  onSelect,
  onOpen,
  query,
  searchLimit,
  onLoadMore,
}: ResultListProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const hasMore = results.length >= searchLimit;

  // Reset scroll to top on query change
  useEffect(() => {
    if (containerRef.current) {
      containerRef.current.scrollTop = 0;
    }
  }, [query]);

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto px-2 pb-2">
      {results.map((result, index) => (
        <ResultItem
          key={result.path + index}
          result={result}
          index={index}
          isActive={index === selectedIndex}
          onSelect={onSelect}
          onOpen={onOpen}
          query={query}
        />
      ))}
      {hasMore && (
        <div
          onClick={onLoadMore}
          className="result-item flex items-center justify-center py-2 px-3.5 border border-dashed border-indigo-500/20 hover:border-indigo-500/35 hover:bg-indigo-500/10 transition-all rounded-lg text-indigo-300 font-semibold cursor-pointer text-[12px] mt-1 select-none shrink-0"
        >
          <span>⏬ Load 50 More Results…</span>
        </div>
      )}
    </div>
  );
}
