import { useEffect, useRef } from 'react';
import { Search, X, Loader2 } from 'lucide-react';

interface TaskSearchBarProps {
  value: string;
  onChange: (value: string) => void;
  onClose: () => void;
  resultCount: number;
  isSearching: boolean;
}

export function TaskSearchBar({
  value,
  onChange,
  onClose,
  resultCount,
  isSearching,
}: TaskSearchBarProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-focus on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const resultText =
    resultCount === 0
      ? 'No results'
      : `${resultCount} task${resultCount === 1 ? '' : 's'} found`;

  return (
    <div
      className="flex h-[30px] items-center gap-2 rounded-md px-2.5"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
        boxShadow: "none",
      }}
    >
      {/* Search icon */}
      <Search className="h-3.5 w-3.5 flex-shrink-0 text-muted-foreground" />

      {/* Input field */}
      <input
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="Search tasks..."
        className="min-w-0 flex-1 bg-transparent border-none outline-none focus:outline-none focus-visible:outline-none focus:ring-0 focus-visible:ring-0 [&:focus]:outline-none [&:focus-visible]:outline-none text-foreground placeholder:text-muted-foreground"
        style={{ outline: 'none', fontSize: '0.7812rem', lineHeight: 1.2 }}
      />

      {/* Loading spinner */}
      {isSearching && (
        <Loader2 className="h-3.5 w-3.5 text-muted-foreground animate-spin" />
      )}

      {/* Result count */}
      {!isSearching && value && (
        <span className="text-[0.6875rem] text-muted-foreground whitespace-nowrap">
          {resultText}
        </span>
      )}

      {/* Close button */}
      <button
        onClick={onClose}
        className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded hover:bg-[var(--bg-hover)] transition-colors"
        aria-label="Close search"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
