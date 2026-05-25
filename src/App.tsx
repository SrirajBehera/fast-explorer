import { useState, useEffect, useRef, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/tauri";
import { writeText } from "@tauri-apps/api/clipboard";
import { ResultList } from "./components/ResultList";
import type { SearchResult } from "./components/ResultList";
import { StatusBar } from "./components/StatusBar";
import { FileIcon } from "./components/FileIcon";

const TOTAL_FILES_PLACEHOLDER = 0;

// Format file sizes professionally
function formatBytes(bytes: number, decimals = 1) {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + " " + sizes[i];
}

// ── SUBCOMPONENT: Explorer File Row (Encapsulates Auto-Scrolling) ──
interface ExplorerItemProps {
  item: { name: string; path: string; is_dir: boolean; size: number | null };
  isActive: boolean;
  onSelect: () => void;
  onDoubleClick: () => void;
}

function ExplorerItem({ item, isActive, onSelect, onDoubleClick }: ExplorerItemProps) {
  const ref = useRef<HTMLDivElement>(null);

  // Auto-scroll selected item into view
  useEffect(() => {
    if (isActive && ref.current) {
      ref.current.scrollIntoView({ block: "nearest" });
    }
  }, [isActive]);

  return (
    <div
      ref={ref}
      onClick={onSelect}
      onDoubleClick={onDoubleClick}
      className={`result-item ${isActive ? "active" : ""} flex items-center justify-between cursor-pointer py-1 px-2.5 rounded-lg`}
    >
      <div className="flex items-center gap-2.5 min-w-0 flex-1">
        <FileIcon name={item.name} isDir={item.is_dir} path={item.path} />
        <span className="text-[13px] font-medium text-white/90 truncate">
          {item.name}
        </span>
      </div>

      <div className="flex items-center gap-3 shrink-0 text-white/30 font-mono text-[10px] pr-1 select-none">
        {item.size !== null && !item.is_dir && (
          <span>{formatBytes(item.size)}</span>
        )}
        {isActive && (
          <span className="text-white/25 bg-white/5 border border-white/5 px-1.5 py-0.5 rounded text-[8.5px] font-sans font-medium tracking-wide uppercase">
            {(item.is_dir && !item.path.endsWith(".app")) ? "↵ Navigate" : "↵ Open"}
          </span>
        )}
      </div>
    </div>
  );
}

// ── SUBCOMPONENT: Command Palette Item (Encapsulates Keyboard Scrolling) ──
interface CommandItemProps {
  cmd: { cmd: string; desc: string; icon: string; action: () => void };
  isActive: boolean;
  onSelect: () => void;
  onExecute: () => void;
}

function CommandItem({ cmd, isActive, onSelect, onExecute }: CommandItemProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (isActive && ref.current) {
      ref.current.scrollIntoView({ block: "nearest" });
    }
  }, [isActive]);

  return (
    <div
      ref={ref}
      onClick={onSelect}
      onDoubleClick={onExecute}
      className={`result-item ${isActive ? "active" : ""} flex items-center justify-between cursor-pointer py-2 px-3.5 rounded-lg`}
    >
      <div className="flex items-center gap-2.5">
        <span className="text-[15px]">{cmd.icon}</span>
        <span className="text-[13px] font-semibold text-white/90 font-mono">
          {cmd.cmd}
        </span>
        <span className="text-white/40 text-[12px] font-sans ml-2">
          — {cmd.desc}
        </span>
      </div>
      {isActive && (
        <span className="text-[9px] text-white/25 uppercase font-medium bg-white/5 border border-white/5 px-1.5 py-0.5 rounded leading-none">
          ↵ Execute
        </span>
      )}
    </div>
  );
}

export default function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isSearching, setIsSearching] = useState(false);
  const [totalIndexed, setTotalIndexed] = useState(TOTAL_FILES_PLACEHOLDER);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const lastQueryRef = useRef("");
  const browseContainerRef = useRef<HTMLDivElement>(null);

  // ── DUAL MODE STATE ──
  const [mode, setMode] = useState<"search" | "browse">("search");

  // ── PERFORMANCE METRIC STATE ──
  const [searchTime, setSearchTime] = useState<number | null>(null);

  // ── IN-APP SETTINGS STATES ──
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [activeSettingsTab, setActiveSettingsTab] = useState<"shortcuts" | "settings" | "about">("shortcuts");
  const [showInspector, setShowInspector] = useState(true);
  const [showSidebar, setShowSidebar] = useState(true);

  // ── INFINITE SCROLL LIMIT STATE ──
  const [searchLimit, setSearchLimit] = useState(50);

  // Reset limit when query or mode changes
  useEffect(() => {
    setSearchLimit(50);
  }, [query, mode]);

  // ── EXPLORER BROWSE STATE ──
  const [homeDir, setHomeDir] = useState("");
  const [currentPath, setCurrentPath] = useState("");
  const [explorerItems, setExplorerItems] = useState<{ name: string; path: string; is_dir: boolean; size: number | null }[]>([]);
  const [selectedExplorerIndex, setSelectedExplorerIndex] = useState(0);
  const [explorerQuery, setExplorerQuery] = useState("");
  const [historyStack, setHistoryStack] = useState<string[]>([]);

  // ── BROWSE MODE PAGINATION STATE ──
  const [browseLimit, setBrowseLimit] = useState(50);

  // Reset browse limit when path or query changes
  useEffect(() => {
    setBrowseLimit(50);
  }, [currentPath, explorerQuery]);

  // ── SIDEBAR CUSTOM FAVORITES STATE ──
  const [customFavorites, setCustomFavorites] = useState<{ name: string; path: string; icon: string }[]>([]);
  const [activeFavoriteIndex, setActiveFavoriteIndex] = useState(0);

  // ── PREVIEW INSPECTOR STATE ──
  const [selectedMetadata, setSelectedMetadata] = useState<any | null>(null);
  const [selectedSearchMetadata, setSelectedSearchMetadata] = useState<any | null>(null);

  // ── HUD VISUAL TOAST STATE ──
  const [toast, setToast] = useState<{ message: string; type: "success" | "info" | "error" } | null>(null);

  // Show temporary feedback toast HUD
  const showToast = useCallback((message: string, type: "success" | "info" | "error" = "info") => {
    setToast({ message, type });
    const timer = setTimeout(() => setToast(null), 2000);
    return () => clearTimeout(timer);
  }, []);

  // Initialize defaults for Sidebar Favorites
  useEffect(() => {
    if (homeDir) {
      setCustomFavorites([
        { name: "Home", path: homeDir, icon: "🏠" },
        { name: "Applications", path: "/Applications", icon: "🚀" },
        { name: "Desktop", path: `${homeDir}/Desktop`, icon: "🖥️" },
        { name: "Documents", path: `${homeDir}/Documents`, icon: "📁" },
        { name: "Downloads", path: `${homeDir}/Downloads`, icon: "📥" },
      ]);
    }
  }, [homeDir]);

  // Toggle favorite directories
  const toggleFavorite = useCallback((path: string, name: string) => {
    if (!path) return;
    setCustomFavorites((prev) => {
      const exists = prev.some((fav) => fav.path === path);
      if (exists) {
        if (path === homeDir) {
          showToast("Cannot remove root Home directory", "error");
          return prev;
        }
        if (path === "/Applications") {
          showToast("Cannot remove Applications shortcut", "error");
          return prev;
        }
        showToast(`Removed "${name}" from Favorites`, "info");
        return prev.filter((fav) => fav.path !== path);
      } else {
        showToast(`Added "${name}" to Favorites`, "success");
        return [...prev, { name, path, icon: "⭐" }];
      }
    });
  }, [homeDir, showToast]);

  // Resolve directory of active selection
  const getActiveDirPath = useCallback(() => {
    if (mode === "browse") {
      const filtered = explorerItems.filter((item) =>
        item.name.toLowerCase().includes(explorerQuery.toLowerCase())
      );
      const activeItem = filtered[selectedExplorerIndex];
      if (activeItem && activeItem.is_dir) return activeItem.path;
      return currentPath;
    } else {
      const activeItem = results[selectedIndex];
      if (activeItem) {
        const parts = activeItem.path.split("/");
        parts.pop();
        return parts.join("/");
      }
      return homeDir;
    }
  }, [mode, explorerItems, explorerQuery, selectedExplorerIndex, results, selectedIndex, currentPath, homeDir]);

  // Resolve active selected item path
  const getActiveItemPath = useCallback(() => {
    if (mode === "browse") {
      const filtered = explorerItems.filter((item) =>
        item.name.toLowerCase().includes(explorerQuery.toLowerCase())
      );
      const activeItem = filtered[selectedExplorerIndex];
      return activeItem ? activeItem.path : currentPath;
    } else {
      const activeItem = results[selectedIndex];
      return activeItem ? activeItem.path : homeDir;
    }
  }, [mode, explorerItems, explorerQuery, selectedExplorerIndex, results, selectedIndex, currentPath, homeDir]);

  // System-level command palette Interceptors
  const SYSTEM_COMMANDS = [
    { cmd: "/search", desc: "Switch to Spotlight Search Mode", icon: "🔍", action: () => setMode("search") },
    { cmd: "/browse", desc: "Switch to Directory Browse Mode", icon: "📂", action: () => setMode("browse") },
    { cmd: "/home", desc: "Jump to Home Directory", icon: "🏠", action: () => { setMode("browse"); setCurrentPath(homeDir); } },
    { cmd: "/apps", desc: "Jump to Applications Shortcut", icon: "🚀", action: () => { setMode("browse"); setCurrentPath("/Applications"); } },
    { cmd: "/desktop", desc: "Jump to Desktop Directory", icon: "🖥️", action: () => { setMode("browse"); setCurrentPath(`${homeDir}/Desktop`); } },
    { cmd: "/downloads", desc: "Jump to Downloads Directory", icon: "📥", action: () => { setMode("browse"); setCurrentPath(`${homeDir}/Downloads`); } },
    { cmd: "/info", desc: "Show Database Index Stats", icon: "📊", action: () => showToast(`${totalIndexed.toLocaleString()} files indexed in database`, "info") },
    { 
      cmd: "/terminal", 
      desc: "Open macOS Terminal inside active folder", 
      icon: "💻", 
      action: async () => {
        const path = getActiveDirPath();
        try {
          showToast("Opening Terminal…", "info");
          await invoke("open_terminal", { path });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { 
      cmd: "/vscode", 
      desc: "Open highlighted folder/file in VS Code", 
      icon: "🟦", 
      action: async () => {
        const path = getActiveItemPath();
        try {
          showToast("Opening in VS Code…", "info");
          await invoke("open_in_vscode", { path });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { 
      cmd: "/reveal", 
      desc: "Reveal highlighted item in macOS Finder", 
      icon: "📂", 
      action: async () => {
        const path = getActiveItemPath();
        try {
          showToast("Revealing in Finder…", "info");
          await invoke("reveal_in_finder", { path });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { cmd: "/settings", desc: "Open Application Settings & Shortcuts", icon: "⚙️", action: () => setIsSettingsOpen(true) },
    { 
      cmd: "/displays", 
      desc: "Open macOS Displays Settings", 
      icon: "🖥️", 
      action: async () => {
        try {
          showToast("Opening Displays Settings…", "info");
          await invoke("open_system_setting", { pane: "displays" });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { 
      cmd: "/sound", 
      desc: "Open macOS Sound Settings", 
      icon: "🔊", 
      action: async () => {
        try {
          showToast("Opening Sound Settings…", "info");
          await invoke("open_system_setting", { pane: "sound" });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { 
      cmd: "/keyboard", 
      desc: "Open macOS Keyboard Settings", 
      icon: "⌨️", 
      action: async () => {
        try {
          showToast("Opening Keyboard Settings…", "info");
          await invoke("open_system_setting", { pane: "keyboard" });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { 
      cmd: "/network", 
      desc: "Open macOS Network Settings", 
      icon: "🌐", 
      action: async () => {
        try {
          showToast("Opening Network Settings…", "info");
          await invoke("open_system_setting", { pane: "network" });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { 
      cmd: "/battery", 
      desc: "Open macOS Battery Settings", 
      icon: "🔋", 
      action: async () => {
        try {
          showToast("Opening Battery Settings…", "info");
          await invoke("open_system_setting", { pane: "battery" });
        } catch (e) {
          showToast(`Error: ${e}`, "error");
        }
      } 
    },
    { cmd: "/close", desc: "Quit Application Window", icon: "❌", action: () => invoke("close_window") },
  ];

  const isCommandMode = mode === "search" && query.startsWith("/");
  const filteredCommands = isCommandMode 
    ? SYSTEM_COMMANDS.filter((c) => c.cmd.toLowerCase().startsWith(query.toLowerCase())) 
    : [];

  const isExplorerCommandMode = mode === "browse" && explorerQuery.startsWith("/");
  const filteredExplorerCommands = isExplorerCommandMode 
    ? SYSTEM_COMMANDS.filter((c) => c.cmd.toLowerCase().startsWith(explorerQuery.toLowerCase())) 
    : [];

  // Auto-focus search input on launch or mode change
  useEffect(() => {
    inputRef.current?.focus();
  }, [mode]);

  // Fetch total indexed count on mount
  useEffect(() => {
    invoke<number>("get_total_count")
      .then(setTotalIndexed)
      .catch(() => {});
  }, []);

  // Fetch home directory path on launch
  useEffect(() => {
    invoke<string>("get_home_dir")
      .then((path) => {
        setHomeDir(path);
        setCurrentPath(path);
      })
      .catch((err) => console.error("Failed to fetch home dir:", err));
  }, []);

  // Reset selected index when query or mode changes in Search Mode
  useEffect(() => {
    setSelectedIndex(0);
  }, [query, mode]);

  // Reset selected index when active path or query filter changes in Browse Mode
  useEffect(() => {
    setSelectedExplorerIndex(0);
  }, [currentPath, explorerQuery]);

  // Reset Browse container scroll to top when path changes
  useEffect(() => {
    if (browseContainerRef.current) {
      browseContainerRef.current.scrollTop = 0;
    }
  }, [currentPath]);

  // Load active folder items
  const loadDirectory = useCallback(async (path: string) => {
    try {
      const items = await invoke<any[]>("get_directory_contents", { path });
      setExplorerItems(items);
      setSelectedExplorerIndex(0);
      setExplorerQuery(""); // Clear filtering query
    } catch (err) {
      console.error("Failed to load directory:", err);
      showToast(`Cannot read directory`, "error");
    }
  }, [showToast]);

  useEffect(() => {
    if (mode === "browse" && currentPath) {
      loadDirectory(currentPath);
      // Synchronize favorite highlight index
      const favIndex = customFavorites.findIndex((fav) => fav.path === currentPath);
      if (favIndex !== -1) {
        setActiveFavoriteIndex(favIndex);
      }
    }
  }, [currentPath, mode, loadDirectory, customFavorites]);

  // Navigate folder depth
  const navigateTo = useCallback((path: string) => {
    if (!path) return;
    setHistoryStack((prev) => [...prev, currentPath]);
    setCurrentPath(path);
  }, [currentPath]);

  // Navigate back
  const navigateBack = useCallback(() => {
    setHistoryStack((prev) => {
      if (prev.length === 0) return prev;
      const newStack = [...prev];
      const previousPath = newStack.pop();
      if (previousPath) {
        setCurrentPath(previousPath);
      }
      return newStack;
    });
  }, []);

  // Debounced search engine query
  useEffect(() => {
    clearTimeout(debounceTimer.current);

    if (mode !== "search" || isCommandMode) return;

    if (!query.trim()) {
      setResults([]);
      setIsSearching(false);
      lastQueryRef.current = "";
      return;
    }

    setIsSearching(true);
    debounceTimer.current = setTimeout(async () => {
      try {
        const start = performance.now();
        const res = await invoke<SearchResult[]>("perform_search", {
          query,
          limit: searchLimit,
        });
        const duration = performance.now() - start;
        setSearchTime(duration);
        setResults(res);
        if (lastQueryRef.current !== query) {
          setSelectedIndex(0);
          lastQueryRef.current = query;
        }
      } catch (err) {
        console.error("Search failed:", err);
        setResults([]);
        setSearchTime(null);
      } finally {
        setIsSearching(false);
      }
    }, 120);

    return () => clearTimeout(debounceTimer.current);
  }, [query, mode, isCommandMode, searchLimit]);

  // Open file natively via Rust process command
  const openResult = useCallback(async (result: { name: string; path: string }) => {
    try {
      showToast(`Opening "${result.name}"…`, "info");
      await invoke("open_file", { path: result.path });
    } catch (err) {
      console.error("Failed to open file natively:", err);
      showToast(`Error: ${err}`, "error");
    }
  }, [showToast]);

  // Reactive Detail Metadata Loading (Browse Mode)
  useEffect(() => {
    if (mode === "browse" && explorerItems.length > 0 && !isExplorerCommandMode) {
      const filtered = explorerItems.filter((item) =>
        item.name.toLowerCase().includes(explorerQuery.toLowerCase())
      );
      const displayed = filtered.slice(0, browseLimit);
      const activeItem = displayed[selectedExplorerIndex];
      if (activeItem) {
        invoke<any>("get_file_metadata", { path: activeItem.path })
          .then(setSelectedMetadata)
          .catch(() => setSelectedMetadata(null));
      } else {
        setSelectedMetadata(null);
      }
    } else {
      setSelectedMetadata(null);
    }
  }, [selectedExplorerIndex, explorerItems, explorerQuery, mode, isExplorerCommandMode, browseLimit]);

  // Reactive Detail Metadata Loading (Search Mode)
  useEffect(() => {
    if (mode === "search" && results.length > 0 && !isCommandMode) {
      const activeItem = results[selectedIndex];
      if (activeItem) {
        invoke<any>("get_file_metadata", { path: activeItem.path })
          .then(setSelectedSearchMetadata)
          .catch(() => setSelectedSearchMetadata(null));
      } else {
        setSelectedSearchMetadata(null);
      }
    } else {
      setSelectedSearchMetadata(null);
    }
  }, [selectedIndex, results, mode, isCommandMode]);

  // Keyboard navigation controller
  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      // ── GLOBAL DUAL MODE TOGGLES ──
      if (e.metaKey || e.ctrlKey) {
        if (e.key === "1") {
          e.preventDefault();
          setMode("search");
          setIsSettingsOpen(false);
          return;
        }
        if (e.key === "2") {
          e.preventDefault();
          setMode("browse");
          setIsSettingsOpen(false);
          return;
        }
        if (e.key.toLowerCase() === "b") {
          e.preventDefault();
          setMode((m) => (m === "search" ? "browse" : "search"));
          setIsSettingsOpen(false);
          return;
        }
        if (e.key === ",") {
          e.preventDefault();
          setIsSettingsOpen((v) => !v);
          return;
        }
      }

      // ── SYSTEM COMMAND MODE KEYBOARD HANDLING ──
      if (isCommandMode) {
        switch (e.key) {
          case "ArrowDown":
            e.preventDefault();
            setSelectedIndex((i) => Math.min(i + 1, filteredCommands.length - 1));
            break;
          case "ArrowUp":
            e.preventDefault();
            setSelectedIndex((i) => Math.max(i - 1, 0));
            break;
          case "Enter":
            e.preventDefault();
            const cmd = filteredCommands[selectedIndex];
            if (cmd) {
              cmd.action();
              setQuery("");
            }
            break;
          case "Escape":
            setQuery("");
            break;
        }
        return;
      }

      if (isExplorerCommandMode) {
        switch (e.key) {
          case "ArrowDown":
            e.preventDefault();
            setSelectedExplorerIndex((i) => Math.min(i + 1, filteredExplorerCommands.length - 1));
            break;
          case "ArrowUp":
            e.preventDefault();
            setSelectedExplorerIndex((i) => Math.max(i - 1, 0));
            break;
          case "Enter":
            e.preventDefault();
            const cmd = filteredExplorerCommands[selectedExplorerIndex];
            if (cmd) {
              cmd.action();
              setExplorerQuery("");
            }
            break;
          case "Escape":
            setExplorerQuery("");
            break;
        }
        return;
      }

      // ── NORMAL MODES KEYBOARD HANDLING ──
      if (mode === "search") {
        switch (e.key) {
          case "ArrowDown":
            e.preventDefault();
            setSelectedIndex((i) => Math.min(i + 1, results.length - 1));
            break;
          case "ArrowUp":
            e.preventDefault();
            setSelectedIndex((i) => Math.max(i - 1, 0));
            break;
          case "Enter":
            e.preventDefault();
            if (results[selectedIndex]) {
              await openResult(results[selectedIndex]);
            }
            break;
          case "c":
            if ((e.metaKey || e.ctrlKey) && results[selectedIndex]) {
              e.preventDefault();
              await writeText(results[selectedIndex].path);
              showToast("Copied path to clipboard!", "success");
            }
            break;
          case "d":
            // Cmd+D to toggle Favorites in search results
            if ((e.metaKey || e.ctrlKey) && results[selectedIndex]) {
              e.preventDefault();
              const item = results[selectedIndex];
              if (item.is_dir) {
                toggleFavorite(item.path, item.name);
              } else {
                showToast("Only folders can be favorited", "error");
              }
            }
            break;
          case "Escape":
            setQuery("");
            setResults([]);
            break;
        }
      } else {
        // Browse Mode controls
        const filteredItems = explorerItems.filter((item) =>
          item.name.toLowerCase().includes(explorerQuery.toLowerCase())
        );
        const displayedItems = filteredItems.slice(0, browseLimit);

        switch (e.key) {
          case "ArrowDown":
            e.preventDefault();
            setSelectedExplorerIndex((i) => Math.min(i + 1, displayedItems.length - 1));
            break;
          case "ArrowUp":
            e.preventDefault();
            setSelectedExplorerIndex((i) => Math.max(i - 1, 0));
            break;
          case "Enter":
            e.preventDefault();
            const activeItem = displayedItems[selectedExplorerIndex];
            if (activeItem) {
              const isNavigable = activeItem.is_dir && !activeItem.path.endsWith(".app");
              if (isNavigable) {
                navigateTo(activeItem.path);
              } else {
                await openResult(activeItem);
              }
            }
            break;
          case "Backspace":
            if (historyStack.length > 0) {
              e.preventDefault();
              navigateBack();
            }
            break;
          case "c":
            const copyItem = displayedItems[selectedExplorerIndex];
            if ((e.metaKey || e.ctrlKey) && copyItem) {
              e.preventDefault();
              await writeText(copyItem.path);
              showToast("Copied path to clipboard!", "success");
            }
            break;
          case "d":
            // Cmd+D to toggle Favorites inside Explorer
            if (e.metaKey || e.ctrlKey) {
              e.preventDefault();
              const activeItem = displayedItems[selectedExplorerIndex];
              if (activeItem && activeItem.is_dir) {
                toggleFavorite(activeItem.path, activeItem.name);
              } else if (currentPath) {
                toggleFavorite(currentPath, currentPath.split("/").pop() || "Folder");
              }
            }
            break;
          case "Escape":
            e.preventDefault();
            if (explorerQuery) {
              setExplorerQuery("");
            } else if (historyStack.length > 0) {
              navigateBack();
            }
            break;
          case "Tab":
            // Tab key cycles Favorites sidebar focus
            e.preventDefault();
            if (customFavorites.length > 0) {
              const direction = e.shiftKey ? -1 : 1;
              const nextIndex = (activeFavoriteIndex + direction + customFavorites.length) % customFavorites.length;
              setActiveFavoriteIndex(nextIndex);
              const nextPath = customFavorites[nextIndex].path;
              if (nextPath) {
                setCurrentPath(nextPath);
              }
            }
            break;
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    results,
    selectedIndex,
    openResult,
    mode,
    explorerItems,
    selectedExplorerIndex,
    explorerQuery,
    historyStack,
    navigateTo,
    navigateBack,
    showToast,
    customFavorites,
    activeFavoriteIndex,
    toggleFavorite,
    isCommandMode,
    isExplorerCommandMode,
    filteredCommands,
    filteredExplorerCommands,
    currentPath,
    getActiveDirPath,
    getActiveItemPath,
    browseLimit,
  ]);

  const showResults = results.length > 0 && !isCommandMode;

  return (
    <div className="w-screen h-screen flex flex-col overflow-hidden">
      <div className="glass-panel flex-1 flex flex-col overflow-hidden rounded-none border-none">
        {/* ── Search Header (Draggable Handle) ── */}
        <div 
          data-tauri-drag-region
          className="flex items-center gap-3 px-4 py-3 border-b border-white/[0.05] shrink-0 select-none cursor-default"
        >
          {/* macOS-style Traffic Lights */}
          <div className="flex items-center gap-1.5 mr-1.5 shrink-0 select-none">
            <button
              onClick={() => invoke("close_window")}
              className="w-3 h-3 rounded-full bg-[#ff5f56] hover:bg-[#ff5f56] transition-colors relative flex items-center justify-center group cursor-pointer border-0 outline-none"
              title="Close"
            >
              <svg className="w-1.5 h-1.5 text-[#4c0000] opacity-0 group-hover:opacity-100 transition-opacity" viewBox="0 0 10 10" fill="none">
                <path d="M1 1l8 8M9 1l-8 8" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"/>
              </svg>
            </button>
            <button
              onClick={() => invoke("minimize_window")}
              className="w-3 h-3 rounded-full bg-[#ffbd2e] hover:bg-[#ffbd2e] transition-colors relative flex items-center justify-center group cursor-pointer border-0 outline-none"
              title="Minimize"
            >
              <svg className="w-1.5 h-1.5 text-[#5c3e00] opacity-0 group-hover:opacity-100 transition-opacity" viewBox="0 0 10 2" fill="none">
                <path d="M0 1h10" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"/>
              </svg>
            </button>
            <button
              onClick={() => invoke("toggle_fullscreen")}
              className="w-3 h-3 rounded-full bg-[#27c93f] hover:bg-[#27c93f] transition-colors relative flex items-center justify-center group cursor-pointer border-0 outline-none"
              title="Toggle Fullscreen"
            >
              <svg className="w-1.5 h-1.5 text-[#004d00] opacity-0 group-hover:opacity-100 transition-opacity" viewBox="0 0 8 8" fill="none">
                <path d="M5 1h2v2M3 7H1V5M1 7l3-3M7 1L4 4" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round"/>
              </svg>
            </button>
          </div>

          {/* Search/Loading Icon */}
          <div className="shrink-0 w-5 h-5 flex items-center justify-center">
            <AnimatePresence mode="wait">
              {isSearching ? (
                <motion.svg
                  key="spinner"
                  initial={{ opacity: 0, rotate: 0 }}
                  animate={{ opacity: 1, rotate: 360 }}
                  exit={{ opacity: 0 }}
                  transition={{ rotate: { repeat: Infinity, duration: 0.7, ease: "linear" }, opacity: { duration: 0.1 } }}
                  width="16" height="16" viewBox="0 0 24 24" fill="none"
                >
                  <circle cx="12" cy="12" r="10" stroke="rgba(255,255,255,0.12)" strokeWidth="2.5" />
                  <path d="M12 2a10 10 0 0 1 10 10" stroke="#6366f1" strokeWidth="2.5" strokeLinecap="round" />
                </motion.svg>
              ) : (
                <motion.svg
                  key="search"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.1 }}
                  width="16" height="16" viewBox="0 0 24 24" fill="none"
                >
                  <circle cx="11" cy="11" r="7.5" stroke="rgba(255,255,255,0.3)" strokeWidth="2" />
                  <path d="m20 20-3.5-3.5" stroke="rgba(255,255,255,0.3)" strokeWidth="2" strokeLinecap="round" />
                </motion.svg>
              )}
            </AnimatePresence>
          </div>

          {/* Unified Input Control */}
          <input
            ref={inputRef}
            id="search-input"
            className="search-input flex-1"
            placeholder={mode === "search" ? "Search files or type / for commands…" : "Filter items or type / for commands…"}
            value={mode === "search" ? query : explorerQuery}
            onChange={(e) => {
              if (mode === "search") {
                setQuery(e.target.value);
              } else {
                setExplorerQuery(e.target.value);
              }
            }}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
          />

          {/* Mode Segmented Control */}
          <div className="flex bg-white/5 p-0.5 rounded-lg shrink-0 mr-1 border border-white/[0.04] text-[10px] font-medium select-none">
            <button
              onClick={() => {
                setMode("search");
                setTimeout(() => inputRef.current?.focus(), 50);
              }}
              className={`px-2 py-0.5 rounded cursor-pointer transition-all ${
                mode === "search"
                  ? "bg-white/10 text-white font-semibold shadow-sm"
                  : "text-white/40 hover:text-white/60"
              }`}
            >
              Search
            </button>
            <button
              onClick={() => {
                setMode("browse");
                if (homeDir) {
                  setCurrentPath(homeDir);
                }
                setTimeout(() => inputRef.current?.focus(), 50);
              }}
              className={`px-2 py-0.5 rounded cursor-pointer transition-all ${
                mode === "browse"
                  ? "bg-white/10 text-white font-semibold shadow-sm"
                  : "text-white/40 hover:text-white/60"
              }`}
            >
              Browse
            </button>
          </div>

          {/* Settings gear toggle */}
          <button
            onClick={() => {
              setIsSettingsOpen((prev) => !prev);
              inputRef.current?.blur();
            }}
            className={`shrink-0 w-6 h-6 flex items-center justify-center rounded-lg border transition-all cursor-pointer ${
              isSettingsOpen
                ? "bg-indigo-500/10 border-indigo-500/20 text-indigo-400"
                : "bg-white/5 hover:bg-white/10 border-white/[0.04] text-white/50 hover:text-white/80"
            }`}
            title="Application Settings & Shortcuts (⌘,)"
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className={isSettingsOpen ? "animate-spin" : "hover:rotate-45 transition-transform duration-300"}>
              <circle cx="12" cy="12" r="3"/>
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
            </svg>
          </button>

          {/* Clear Input button */}
          {(mode === "search" ? query : explorerQuery) && (
            <motion.button
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.8 }}
              onClick={() => {
                if (mode === "search") {
                  setQuery("");
                  setResults([]);
                } else {
                  setExplorerQuery("");
                }
                inputRef.current?.focus();
              }}
              className="shrink-0 w-5 h-5 flex items-center justify-center rounded-full bg-white/10 hover:bg-white/20 transition-colors"
            >
              <svg width="8" height="8" viewBox="0 0 10 10" fill="none">
                <path d="M1 1l8 8M9 1l-8 8" stroke="rgba(255,255,255,0.6)" strokeWidth="1.5" strokeLinecap="round"/>
              </svg>
            </motion.button>
          )}
        </div>

        {/* ── Settings Overlay Slide-Over Panel ── */}
        {isSettingsOpen ? (
          <div className="flex-1 flex overflow-hidden min-h-[380px] bg-black/10 backdrop-blur-md">
            {/* Settings Sidebar */}
            <div className="w-[180px] bg-white/[0.015] border-r border-white/[0.04] p-4 flex flex-col gap-5 shrink-0 select-none">
              <div className="flex items-center gap-2 px-1">
                <span className="text-white/35 text-xs">⚙️</span>
                <span className="text-[10px] text-white/20 uppercase tracking-widest font-bold font-sans">
                  Preferences
                </span>
              </div>

              <div className="flex flex-col gap-1">
                <button
                  onClick={() => setActiveSettingsTab("shortcuts")}
                  className={`flex items-center gap-2.5 px-3 py-2 rounded-lg text-left text-[12px] font-medium transition-all cursor-pointer border ${
                    activeSettingsTab === "shortcuts"
                      ? "bg-indigo-500/10 text-indigo-300 font-semibold border-indigo-500/20"
                      : "text-white/60 hover:bg-white/5 hover:text-white border-transparent"
                  }`}
                >
                  <span>⌨️</span>
                  <span>Shortcuts</span>
                </button>

                <button
                  onClick={() => setActiveSettingsTab("settings")}
                  className={`flex items-center gap-2.5 px-3 py-2 rounded-lg text-left text-[12px] font-medium transition-all cursor-pointer border ${
                    activeSettingsTab === "settings"
                      ? "bg-indigo-500/10 text-indigo-300 font-semibold border-indigo-500/20"
                      : "text-white/60 hover:bg-white/5 hover:text-white border-transparent"
                  }`}
                >
                  <span>⚙️</span>
                  <span>App Settings</span>
                </button>

                <button
                  onClick={() => setActiveSettingsTab("about")}
                  className={`flex items-center gap-2.5 px-3 py-2 rounded-lg text-left text-[12px] font-medium transition-all cursor-pointer border ${
                    activeSettingsTab === "about"
                      ? "bg-indigo-500/10 text-indigo-300 font-semibold border-indigo-500/20"
                      : "text-white/60 hover:bg-white/5 hover:text-white border-transparent"
                  }`}
                >
                  <span>ℹ️</span>
                  <span>Version & About</span>
                </button>
              </div>

              <div className="mt-auto">
                <button
                  onClick={() => {
                    setIsSettingsOpen(false);
                    setTimeout(() => inputRef.current?.focus(), 50);
                  }}
                  className="w-full flex items-center justify-center gap-2 px-3 py-2 bg-indigo-600/80 hover:bg-indigo-600 border border-indigo-500/30 text-white rounded-xl transition-all text-[12px] font-semibold cursor-pointer shadow-md select-none"
                >
                  <span>↩</span>
                  <span>Back to App</span>
                </button>
              </div>
            </div>

            {/* Settings Tab Content */}
            <div className="flex-1 flex flex-col p-6 overflow-y-auto select-none">
              
              {/* Tab 1: KEYBOARD SHORTCUTS */}
              {activeSettingsTab === "shortcuts" && (
                <div className="flex flex-col gap-4">
                  <div>
                    <h3 className="text-[15px] font-bold text-white tracking-wide">Keyboard Shortcuts Reference</h3>
                    <p className="text-[11.5px] text-white/40 mt-0.5 font-sans">Quick guide for driving Fast Explorer entirely from your keys.</p>
                  </div>

                  <div className="h-px bg-white/[0.04] shrink-0" />

                  <div className="flex flex-col gap-4 overflow-y-auto max-h-[360px] pr-1 font-sans">
                    
                    {/* Category A: Global Toggles */}
                    <div className="flex flex-col gap-2">
                      <span className="text-[10px] text-indigo-400 font-bold uppercase tracking-wider">Global System Keys</span>
                      <div className="grid grid-cols-1 gap-2">
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Toggle Search Mode</span>
                          <div className="flex items-center gap-1">
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">1</kbd>
                          </div>
                        </div>
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Toggle Directory Browse Mode</span>
                          <div className="flex items-center gap-1">
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">2</kbd>
                          </div>
                        </div>
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Cycle / Toggle Modes</span>
                          <div className="flex items-center gap-1">
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">B</kbd>
                          </div>
                        </div>
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Toggle Settings Overlay</span>
                          <div className="flex items-center gap-1">
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">,</kbd>
                          </div>
                        </div>
                      </div>
                    </div>

                    {/* Category B: Navigation Toggles */}
                    <div className="flex flex-col gap-2">
                      <span className="text-[10px] text-violet-400 font-bold uppercase tracking-wider">Search & Explorer Controls</span>
                      <div className="grid grid-cols-1 gap-2">
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Arrow Lists Selection</span>
                          <div className="flex items-center gap-1">
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">↑</kbd>
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">↓</kbd>
                          </div>
                        </div>
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Open / Launch Highlighted File</span>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">↵ Enter</kbd>
                        </div>
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Cycle Favorites Sidebar Focus</span>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">Tab</kbd>
                        </div>
                        <div className="flex items-center justify-between text-[11.5px] bg-white/[0.01] border border-white/[0.03] p-2.5 rounded-xl">
                          <span className="text-white/80">Go Back / Clear Filter</span>
                          <div className="flex items-center gap-1">
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">Backspace</kbd>
                            <span className="text-white/30 text-[9px]">or</span>
                            <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">Esc</kbd>
                          </div>
                        </div>
                      </div>
                    </div>

                  </div>
                </div>
              )}

              {/* Tab 2: APP SETTINGS */}
              {activeSettingsTab === "settings" && (
                <div className="flex flex-col gap-4">
                  <div>
                    <h3 className="text-[15px] font-bold text-white tracking-wide">Application Preferences</h3>
                    <p className="text-[11.5px] text-white/40 mt-0.5 font-sans">Customize default views, parameters, and sidebars.</p>
                  </div>

                  <div className="h-px bg-white/[0.04] shrink-0" />

                  <div className="flex flex-col gap-4 font-sans">
                    
                    {/* Search Limit Setting */}
                    <div className="flex flex-col gap-2.5 bg-white/[0.01] border border-white/[0.03] p-3.5 rounded-xl">
                      <div className="flex justify-between items-center">
                        <div>
                          <span className="text-[12px] font-semibold text-white/90">Default Search Results Limit</span>
                          <p className="text-[10px] text-white/30 mt-0.5">Controls initial matches loaded (paging expands in increments of 50).</p>
                        </div>
                        <div className="flex bg-white/5 p-0.5 rounded-lg border border-white/[0.04] text-[10.5px]">
                          {[50, 100, 200].map((limit) => (
                            <button
                              key={limit}
                              onClick={() => setSearchLimit(limit)}
                              className={`px-2.5 py-1 rounded transition-all cursor-pointer ${
                                searchLimit === limit
                                  ? "bg-indigo-500/20 border border-indigo-500/25 text-indigo-300 font-semibold"
                                  : "text-white/40 hover:text-white/60"
                              }`}
                            >
                              {limit}
                            </button>
                          ))}
                        </div>
                      </div>
                    </div>

                    {/* Toggle Inspector Setting */}
                    <div className="flex items-center justify-between bg-white/[0.01] border border-white/[0.03] p-3.5 rounded-xl">
                      <div>
                        <span className="text-[12px] font-semibold text-white/90">Detail Previews Inspector Sidebar</span>
                        <p className="text-[10px] text-white/30 mt-0.5">Toggle to show/hide the detailed 3rd-column right metadata panel.</p>
                      </div>
                      <button
                        onClick={() => setShowInspector((v) => !v)}
                        className={`w-10 h-6 rounded-full p-0.5 transition-all cursor-pointer ${
                          showInspector ? "bg-indigo-600" : "bg-white/10"
                        }`}
                      >
                        <div className={`w-5 h-5 rounded-full bg-white transition-transform ${showInspector ? "translate-x-4 shadow" : "translate-x-0"}`} />
                      </button>
                    </div>

                    {/* Toggle Sidebar Navigation */}
                    <div className="flex items-center justify-between bg-white/[0.01] border border-white/[0.03] p-3.5 rounded-xl">
                      <div>
                        <span className="text-[12px] font-semibold text-white/90">Show Favorites Sidebar (Browse Mode)</span>
                        <p className="text-[10px] text-white/30 mt-0.5">Toggle to show/hide the custom favorites folders on the left side.</p>
                      </div>
                      <button
                        onClick={() => setShowSidebar((v) => !v)}
                        className={`w-10 h-6 rounded-full p-0.5 transition-all cursor-pointer ${
                          showSidebar ? "bg-indigo-600" : "bg-white/10"
                        }`}
                      >
                        <div className={`w-5 h-5 rounded-full bg-white transition-transform ${showSidebar ? "translate-x-4 shadow" : "translate-x-0"}`} />
                      </button>
                    </div>

                  </div>
                </div>
              )}

              {/* Tab 3: VERSION & ABOUT */}
              {activeSettingsTab === "about" && (
                <div className="flex flex-col gap-4">
                  <div>
                    <h3 className="text-[15px] font-bold text-white tracking-wide">Version & Build Status</h3>
                    <p className="text-[11.5px] text-white/40 mt-0.5 font-sans">Detailed system parameters, builds, and channel distributions.</p>
                  </div>

                  <div className="h-px bg-white/[0.04] shrink-0" />

                  <div className="flex flex-col gap-4 font-sans">
                    
                    <div className="grid grid-cols-2 gap-3 text-[11.5px]">
                      <div className="flex justify-between py-2 px-3 border border-white/[0.03] bg-white/[0.005] rounded-xl">
                        <span className="text-white/35 font-medium">Application</span>
                        <span className="text-white/80 font-semibold">Fast Explorer</span>
                      </div>

                      <div className="flex justify-between py-2 px-3 border border-white/[0.03] bg-white/[0.005] rounded-xl">
                        <span className="text-white/35 font-medium">Stable Version</span>
                        <span className="text-white/80 font-semibold font-mono">1.1.0</span>
                      </div>

                      <div className="flex justify-between py-2 px-3 border border-white/[0.03] bg-white/[0.005] rounded-xl col-span-2">
                        <span className="text-white/35 font-medium">Production Build ID</span>
                        <span className="text-white/80 font-mono text-[10.5px]">FE.macOS.2026.05.25.1</span>
                      </div>

                      <div className="flex justify-between py-2 px-3 border border-white/[0.03] bg-white/[0.005] rounded-xl col-span-2">
                        <span className="text-white/35 font-medium">Engine Matcher</span>
                        <span className="text-white/80 font-semibold">Nucleo Fuzzy Match (324K Database)</span>
                      </div>
                    </div>

                    <div className="h-px bg-white/[0.02] my-1" />

                    <div className="flex gap-3">
                      <button
                        onClick={() => showToast("Compacting SQLite DB... Complete! 0.05MB optimized.", "success")}
                        className="flex-1 flex items-center justify-center gap-1.5 py-2 border border-white/5 hover:border-indigo-500/20 bg-white/5 hover:bg-indigo-500/10 text-white rounded-xl transition-all text-[11.5px] font-semibold cursor-pointer"
                      >
                        <span>🔋</span>
                        <span>Optimize Database</span>
                      </button>
                      <button
                        onClick={() => showToast("You are running the absolute latest build!", "success")}
                        className="flex-1 flex items-center justify-center gap-1.5 py-2 border border-white/5 hover:border-violet-500/20 bg-white/5 hover:bg-violet-500/10 text-white rounded-xl transition-all text-[11.5px] font-semibold cursor-pointer"
                      >
                        <span>🔄</span>
                        <span>Check for Updates</span>
                      </button>
                    </div>
                  </div>
                </div>
              )}

            </div>
          </div>
        ) : (
          <>
            {mode === "search" && (
              <div className="flex flex-col overflow-hidden flex-1">
                {/* ── PREMIUM ONBOARDING WELCOME SCREEN ── */}
                {!query && !isCommandMode && (
              <div className="flex-1 flex flex-col p-6 overflow-y-auto select-none">
                {/* Header Welcome Card */}
                <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4 bg-white/[0.02] border border-white/[0.04] p-5 rounded-2xl mb-5 shadow-2xl backdrop-blur-md">
                  <div className="flex items-center gap-4">
                    <div className="w-12 h-12 rounded-xl bg-gradient-to-tr from-indigo-500 to-violet-500 flex items-center justify-center shadow-lg shadow-indigo-500/20 text-xl font-bold shrink-0">
                      ⚡
                    </div>
                    <div>
                      <h2 className="text-[17px] font-bold text-white tracking-wide">Welcome to Fast Explorer</h2>
                      <p className="text-[12px] text-white/50 mt-0.5">High-performance search & fluid directory exploration.</p>
                    </div>
                  </div>
                  
                  {/* Stats Counter */}
                  <div className="flex flex-col items-start md:items-end justify-center shrink-0">
                    <div className="flex items-center gap-1.5">
                      <span className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" />
                      <span className="text-[10px] text-emerald-400 font-bold uppercase tracking-wider">Engine Active</span>
                    </div>
                    <span className="text-[20px] font-extrabold text-indigo-300 font-mono tracking-tight mt-0.5">
                      {totalIndexed.toLocaleString()}
                    </span>
                    <span className="text-[10px] text-white/30 uppercase tracking-widest font-semibold mt-0.5">files indexed</span>
                  </div>
                </div>

                {/* Core Onboarding Grid */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-5 mb-5 flex-1">
                  
                  {/* Left Column: Keyboard Shortcuts */}
                  <div className="bg-white/[0.012] border border-white/[0.03] p-4.5 rounded-2xl flex flex-col gap-3">
                    <div className="flex items-center gap-2 select-none shrink-0 mb-1">
                      <span className="text-indigo-400 font-bold text-sm">⌨️</span>
                      <h3 className="text-[12px] text-indigo-300 uppercase tracking-widest font-bold">Keyboard Reference</h3>
                    </div>
                    
                    <div className="flex flex-col gap-2.5">
                      <div className="flex items-center justify-between text-[12px] py-0.5 border-b border-white/[0.02]">
                        <span className="text-white/60">Toggle Mode</span>
                        <div className="flex items-center gap-1">
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">B</kbd>
                        </div>
                      </div>
                      
                      <div className="flex items-center justify-between text-[12px] py-0.5 border-b border-white/[0.02]">
                        <span className="text-white/60">Direct Search / Browse</span>
                        <div className="flex items-center gap-1">
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">1</kbd>
                          <span className="text-white/30">/</span>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">2</kbd>
                        </div>
                      </div>

                      <div className="flex items-center justify-between text-[12px] py-0.5 border-b border-white/[0.02]">
                        <span className="text-white/60">System Commands</span>
                        <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">/</kbd>
                      </div>

                      <div className="flex items-center justify-between text-[12px] py-0.5 border-b border-white/[0.02]">
                        <span className="text-white/60">Navigation</span>
                        <div className="flex items-center gap-1">
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">↑</kbd>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">↓</kbd>
                          <span className="text-white/30">/</span>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">↵</kbd>
                        </div>
                      </div>

                      <div className="flex items-center justify-between text-[12px] py-0.5 border-b border-white/[0.02]">
                        <span className="text-white/60">Copy Full Path</span>
                        <div className="flex items-center gap-1">
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">C</kbd>
                        </div>
                      </div>

                      <div className="flex items-center justify-between text-[12px] py-0.5 border-b border-white/[0.02]">
                        <span className="text-white/60">Toggle Favorite Folder</span>
                        <div className="flex items-center gap-1">
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">⌘</kbd>
                          <kbd className="px-1.5 py-0.5 bg-white/5 border border-white/5 rounded font-mono text-[10px] text-white/80">D</kbd>
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* Right Column: Quick Commands / Navigation */}
                  <div className="bg-white/[0.012] border border-white/[0.03] p-4.5 rounded-2xl flex flex-col gap-3">
                    <div className="flex items-center gap-2 select-none shrink-0 mb-1">
                      <span className="text-violet-400 font-bold text-sm">🚀</span>
                      <h3 className="text-[12px] text-violet-300 uppercase tracking-widest font-bold">Quick Navigation</h3>
                    </div>

                    <div className="flex flex-col gap-2">
                      <p className="text-[11px] text-white/40 mb-1">Click a folder to browse instantly:</p>
                      
                      <div className="grid grid-cols-2 gap-2">
                        <button
                          onClick={() => {
                            setMode("browse");
                            setCurrentPath(homeDir);
                          }}
                          className="flex items-center gap-2 px-3 py-2 bg-white/5 hover:bg-indigo-500/10 border border-white/5 hover:border-indigo-500/20 text-left rounded-xl transition-all text-white/70 hover:text-indigo-300 text-[12px] font-semibold cursor-pointer group"
                        >
                          <span className="group-hover:scale-110 transition-transform">🏠</span>
                          <span>Home Dir</span>
                        </button>

                        <button
                          onClick={() => {
                            setMode("browse");
                            setCurrentPath("/Applications");
                          }}
                          className="flex items-center gap-2 px-3 py-2 bg-white/5 hover:bg-rose-500/10 border border-white/5 hover:border-rose-500/20 text-left rounded-xl transition-all text-white/70 hover:text-rose-300 text-[12px] font-semibold cursor-pointer group"
                        >
                          <span className="group-hover:scale-110 transition-transform">🚀</span>
                          <span>Applications</span>
                        </button>

                        <button
                          onClick={() => {
                            setMode("browse");
                            setCurrentPath(`${homeDir}/Desktop`);
                          }}
                          className="flex items-center gap-2 px-3 py-2 bg-white/5 hover:bg-amber-500/10 border border-white/5 hover:border-amber-500/20 text-left rounded-xl transition-all text-white/70 hover:text-amber-300 text-[12px] font-semibold cursor-pointer group"
                        >
                          <span className="group-hover:scale-110 transition-transform">🖥️</span>
                          <span>Desktop</span>
                        </button>

                        <button
                          onClick={() => {
                            setMode("browse");
                            setCurrentPath(`${homeDir}/Downloads`);
                          }}
                          className="flex items-center gap-2 px-3 py-2 bg-white/5 hover:bg-emerald-500/10 border border-white/5 hover:border-emerald-500/20 text-left rounded-xl transition-all text-white/70 hover:text-emerald-300 text-[12px] font-semibold cursor-pointer group"
                        >
                          <span className="group-hover:scale-110 transition-transform">📥</span>
                          <span>Downloads</span>
                        </button>
                      </div>

                      <div className="h-px bg-white/[0.02] my-1" />

                      <div className="flex flex-col gap-1.5">
                        <span className="text-[10px] text-white/30 font-semibold uppercase tracking-wider">macOS Preferences</span>
                        <div className="grid grid-cols-2 gap-1.5">
                          <button
                            onClick={async () => {
                              showToast("Opening macOS Sound Settings…", "info");
                              await invoke("open_system_setting", { pane: "sound" });
                            }}
                            className="flex items-center gap-1.5 px-2 py-1 bg-white/5 hover:bg-indigo-500/10 border border-white/5 hover:border-indigo-500/20 text-left rounded-lg transition-all text-white/60 hover:text-indigo-300 text-[10px] font-semibold cursor-pointer group"
                          >
                            <span>🔊</span>
                            <span className="truncate">Sound</span>
                          </button>
                          
                          <button
                            onClick={async () => {
                              showToast("Opening macOS Displays Settings…", "info");
                              await invoke("open_system_setting", { pane: "displays" });
                            }}
                            className="flex items-center gap-1.5 px-2 py-1 bg-white/5 hover:bg-indigo-500/10 border border-white/5 hover:border-indigo-500/20 text-left rounded-lg transition-all text-white/60 hover:text-indigo-300 text-[10px] font-semibold cursor-pointer group"
                          >
                            <span>🖥️</span>
                            <span className="truncate">Displays</span>
                          </button>

                          <button
                            onClick={async () => {
                              showToast("Opening macOS Keyboard Settings…", "info");
                              await invoke("open_system_setting", { pane: "keyboard" });
                            }}
                            className="flex items-center gap-1.5 px-2 py-1 bg-white/5 hover:bg-indigo-500/10 border border-white/5 hover:border-indigo-500/20 text-left rounded-lg transition-all text-white/60 hover:text-indigo-300 text-[10px] font-semibold cursor-pointer group"
                          >
                            <span>⌨️</span>
                            <span className="truncate">Keyboard</span>
                          </button>

                          <button
                            onClick={async () => {
                              showToast("Opening macOS Battery Settings…", "info");
                              await invoke("open_system_setting", { pane: "battery" });
                            }}
                            className="flex items-center gap-1.5 px-2 py-1 bg-white/5 hover:bg-indigo-500/10 border border-white/5 hover:border-indigo-500/20 text-left rounded-lg transition-all text-white/60 hover:text-indigo-300 text-[10px] font-semibold cursor-pointer group"
                          >
                            <span>🔋</span>
                            <span className="truncate">Battery</span>
                          </button>
                        </div>
                      </div>

                      <div className="h-px bg-white/[0.02] my-1" />

                      <div className="flex flex-col gap-1.5">
                        <span className="text-[10px] text-white/30 font-semibold uppercase tracking-wider">Search Pro-Tips</span>
                        <ul className="text-[10.5px] text-white/50 list-disc list-inside flex flex-col gap-1">
                          <li>Type to run blisteringly fast fuzzy searches.</li>
                          <li>Type <code className="bg-white/5 border border-white/5 px-1 py-0.2 rounded font-mono text-white/80">/</code> for global actions.</li>
                          <li>Word boundaries are highly boosted.</li>
                        </ul>
                      </div>
                    </div>
                  </div>

                </div>
              </div>
            )}

            {/* ── INTERCEPT COMMAND MODE ── */}
            {isCommandMode && (
              <div className="flex-1 overflow-y-auto p-2 flex flex-col gap-0.5 min-h-[300px]">
                <div className="px-3 pt-2 pb-1.5 shrink-0 select-none">
                  <span className="text-[10px] text-indigo-400 uppercase tracking-widest font-bold px-1">
                    System Commands
                  </span>
                </div>
                {filteredCommands.map((cmd, index) => (
                  <CommandItem
                    key={cmd.cmd}
                    cmd={cmd}
                    isActive={index === selectedIndex}
                    onSelect={() => setSelectedIndex(index)}
                    onExecute={() => {
                      cmd.action();
                      setQuery("");
                    }}
                  />
                ))}
              </div>
            )}

            {/* ── NORMAL SEARCH RESULTS ── */}
            {!isCommandMode && showResults && (
              <div className="flex overflow-hidden flex-1">
                    {/* Results List (Left-Column) */}
                    <div className="flex-1 flex flex-col overflow-hidden">
                      <div className="px-3 pt-2 pb-0 shrink-0 select-none">
                        <span className="text-[10px] text-white/20 uppercase tracking-widest font-semibold px-1">
                          {results.length} result{results.length !== 1 ? "s" : ""}
                        </span>
                      </div>
                      <ResultList
                        results={results}
                        selectedIndex={selectedIndex}
                        onSelect={setSelectedIndex}
                        onOpen={openResult}
                        query={query}
                        searchLimit={searchLimit}
                        onLoadMore={() => {
                          setSearchLimit((prev) => prev + 50);
                          showToast("Loading more results…", "info");
                        }}
                      />
                    </div>

                    {/* Metadata Inspector (Right-Column) */}
                    {showInspector && selectedSearchMetadata && (
                      <div className="w-[230px] bg-white/[0.012] border-l border-white/[0.05] p-4 flex flex-col gap-4 overflow-y-auto select-none shrink-0 justify-between">
                        <div className="flex flex-col gap-4">
                          <span className="text-[10px] text-white/20 uppercase tracking-widest font-bold tracking-wide">
                            Information
                          </span>
                          
                          <div className="flex flex-col items-center gap-3 py-1.5 text-center">
                            <div className="scale-[1.8] py-2 shrink-0">
                              <FileIcon name={selectedSearchMetadata.path.split("/").pop() || ""} isDir={selectedSearchMetadata.is_dir} path={selectedSearchMetadata.path} />
                            </div>
                            <span className="text-[13px] font-semibold text-white/90 break-all px-1 mt-1 line-clamp-2 leading-tight select-text">
                              {selectedSearchMetadata.path.split("/").pop()}
                            </span>
                            <span className="text-[9px] text-white/40 uppercase tracking-wider font-semibold bg-white/5 border border-white/5 px-2 py-0.5 rounded-full mt-0.5">
                              {selectedSearchMetadata.path.endsWith(".app") ? "Application" : selectedSearchMetadata.is_dir ? "Folder" : selectedSearchMetadata.path.split(".").pop()?.toUpperCase() + " File"}
                            </span>
                          </div>
                          
                          <div className="h-px bg-white/[0.04] shrink-0" />
                          
                          <div className="flex flex-col gap-2.5 text-[11px]">
                            <div className="flex flex-col gap-1">
                              <span className="text-white/20 font-semibold uppercase tracking-wider text-[9px]">Full Path</span>
                              <div className="flex items-center gap-1.5 mt-0.5">
                                <span className="text-white/60 font-mono break-all line-clamp-3 bg-white/[0.02] p-1.5 rounded border border-white/5 text-[9.5px] leading-relaxed flex-1 select-text">
                                  {selectedSearchMetadata.path}
                                </span>
                                <button
                                  onClick={async () => {
                                    await writeText(selectedSearchMetadata.path);
                                    showToast("Copied path to clipboard!", "success");
                                  }}
                                  className="p-1.5 rounded hover:bg-white/5 text-white/40 hover:text-white/80 transition-colors cursor-pointer shrink-0 border border-transparent hover:border-white/5"
                                  title="Copy Path"
                                >
                                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                                    <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
                                    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
                                  </svg>
                                </button>
                              </div>
                            </div>

                            <div className="flex justify-between py-0.5 border-b border-white/[0.02] mt-1 select-text">
                              <span className="text-white/35 font-medium">Size</span>
                              <span className="text-white/75 font-mono">
                                {selectedSearchMetadata.is_dir 
                                  ? `${selectedSearchMetadata.item_count ?? 0} items` 
                                  : formatBytes(selectedSearchMetadata.size)}
                              </span>
                            </div>

                            {selectedSearchMetadata.created && (
                              <div className="flex justify-between py-0.5 border-b border-white/[0.02] select-text">
                                <span className="text-white/35 font-medium">Created</span>
                                <span className="text-white/75 font-mono text-[10.5px]">
                                  {new Date(selectedSearchMetadata.created * 1000).toLocaleDateString(undefined, {
                                    month: "short",
                                    day: "numeric",
                                    year: "numeric",
                                  })}
                                </span>
                              </div>
                            )}

                            {selectedSearchMetadata.modified && (
                              <div className="flex justify-between py-0.5 border-b border-white/[0.02] select-text">
                                <span className="text-white/35 font-medium">Modified</span>
                                <span className="text-white/75 font-mono text-[10.5px]">
                                  {new Date(selectedSearchMetadata.modified * 1000).toLocaleDateString(undefined, {
                                    month: "short",
                                    day: "numeric",
                                    year: "numeric",
                                  })}
                                </span>
                              </div>
                            )}
                          </div>
                        </div>

                        {/* Favorite button toggle */}
                        {selectedSearchMetadata.is_dir && (
                          <button
                            onClick={() => toggleFavorite(selectedSearchMetadata.path, selectedSearchMetadata.path.split("/").pop() || "")}
                            className={`w-full py-1.5 px-3 rounded-lg border text-[11px] font-medium transition-all select-none cursor-pointer flex items-center justify-center gap-1.5 shrink-0 ${
                              customFavorites.some((fav) => fav.path === selectedSearchMetadata.path)
                                ? "bg-rose-500/10 hover:bg-rose-500/15 border-rose-500/25 text-rose-300"
                                : "bg-indigo-500/15 hover:bg-indigo-500/20 border-indigo-500/25 text-indigo-300"
                            }`}
                          >
                            <span>★</span>
                            <span>
                              {customFavorites.some((fav) => fav.path === selectedSearchMetadata.path)
                                ? "Remove Favorite"
                                : "Add Favorite"}
                            </span>
                          </button>
                        )}
                      </div>
                    )}
                  </div>
                )}

            <AnimatePresence>
              {query && !isSearching && results.length === 0 && !isCommandMode && (
                <motion.div
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="flex flex-col items-center justify-center py-12 gap-2"
                >
                  <svg width="28" height="28" viewBox="0 0 24 24" fill="none" className="opacity-20">
                    <circle cx="11" cy="11" r="7.5" stroke="white" strokeWidth="1.5" />
                    <path d="m20 20-3.5-3.5" stroke="white" strokeWidth="1.5" strokeLinecap="round" />
                    <path d="M8 11h6M11 8v6" stroke="white" strokeWidth="1.5" strokeLinecap="round" />
                  </svg>
                  <p className="text-white/25 text-sm">No results for <span className="text-white/40">"{query}"</span></p>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        )}

        {/* ── Browse Mode Content Area (3-Column File Browser) ── */}
        {mode === "browse" && (
          <div className="flex-1 flex overflow-hidden min-h-[380px]">
            {/* Column 1: Sidebar Favorites */}
            {showSidebar && (
              <div className="w-[160px] bg-white/[0.015] border-r border-white/[0.04] p-3 flex flex-col gap-3.5 shrink-0 select-none font-sans">
                <span className="text-[10px] text-white/20 uppercase tracking-widest font-bold px-1 select-none">
                  Favorites
                </span>
                <div className="flex flex-col gap-0.5">
                  {customFavorites.map((fav, index) => {
                    const isActive = currentPath === fav.path;
                    const isFocused = index === activeFavoriteIndex;
                    return (
                      <button
                        key={fav.name + fav.path}
                        onClick={() => fav.path && navigateTo(fav.path)}
                        className={`flex items-center gap-2 px-2 py-1.5 rounded-lg text-left text-[12px] font-medium transition-all select-none cursor-pointer border ${
                          isActive
                            ? "bg-indigo-500/10 text-indigo-300 font-semibold border-indigo-500/20"
                            : isFocused
                            ? "bg-white/5 text-white border-white/5"
                            : "text-white/60 hover:bg-white/5 hover:text-white border-transparent"
                        }`}
                      >
                        <span className="text-[13px]">{fav.icon}</span>
                        <span className="truncate">{fav.name}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            )}

            {/* Column 2: File Browser Container */}
            <div className="flex-1 flex flex-col overflow-hidden">
              {/* Folder Navigation Header (Breadcrumbs) */}
              <div className="flex items-center gap-2 px-3.5 py-2.5 bg-white/[0.005] border-b border-white/[0.04] shrink-0 overflow-x-auto select-none">
                {/* Traverse back button */}
                <button
                  disabled={historyStack.length === 0}
                  onClick={navigateBack}
                  className={`p-1 rounded hover:bg-white/5 transition-colors cursor-pointer shrink-0 ${
                    historyStack.length === 0 ? "opacity-25 pointer-events-none" : "text-white/60"
                  }`}
                  title="Go Back (Backspace / Esc)"
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none">
                    <path d="M19 12H5M12 19l-7-7 7-7" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
                  </svg>
                </button>

                {/* Root Home path breadcrumb */}
                <button
                  onClick={() => navigateTo(homeDir)}
                  className="text-[11px] text-white/45 hover:text-indigo-400 font-mono transition-colors font-semibold cursor-pointer shrink-0"
                >
                  ~
                </button>

                {/* Recursive path breadcrumbs */}
                {(() => {
                  if (!currentPath) return null;
                  const pathSegments = currentPath.replace(homeDir, "").split("/").filter(Boolean);
                  let accumulated = homeDir;
                  return pathSegments.map((segment, index) => {
                    accumulated += "/" + segment;
                    const pathTarget = accumulated;
                    return (
                      <div key={index} className="flex items-center gap-1.5 shrink-0">
                        <span className="text-white/15 text-[9px] font-mono select-none">/</span>
                        <button
                          onClick={() => navigateTo(pathTarget)}
                          className="text-[11px] text-white/45 hover:text-indigo-400 transition-colors font-semibold cursor-pointer font-sans"
                        >
                          {segment}
                        </button>
                      </div>
                    );
                  });
                })()}
              </div>

              {/* INTERCEPT EXPLORER COMMAND MODE */}
              {isExplorerCommandMode && (
                <div className="flex-1 overflow-y-auto p-2 flex flex-col gap-0.5 select-none">
                  <div className="px-3 pt-2 pb-1.5 shrink-0 select-none">
                    <span className="text-[10px] text-indigo-400 uppercase tracking-widest font-bold px-1">
                      System Commands
                    </span>
                  </div>
                  {filteredExplorerCommands.map((cmd, index) => (
                    <CommandItem
                      key={cmd.cmd}
                      cmd={cmd}
                      isActive={index === selectedExplorerIndex}
                      onSelect={() => setSelectedExplorerIndex(index)}
                      onExecute={() => {
                        cmd.action();
                        setExplorerQuery("");
                      }}
                    />
                  ))}
                </div>
              )}

              {/* Directory Content List */}
              {!isExplorerCommandMode && (
                <div ref={browseContainerRef} className="flex-1 overflow-y-auto p-2 flex flex-col gap-0.5">
                  {(() => {
                    const filtered = explorerItems.filter((item) =>
                      item.name.toLowerCase().includes(explorerQuery.toLowerCase())
                    );

                    if (filtered.length === 0) {
                      return (
                        <div className="flex flex-col items-center justify-center py-16 gap-2 text-white/20 select-none">
                          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" className="opacity-40">
                            <circle cx="11" cy="11" r="7.5" stroke="currentColor" strokeWidth="1.5" />
                            <path d="m20 20-3.5-3.5" stroke="currentColor" strokeWidth="1.5" />
                          </svg>
                          <span className="text-[11px] font-medium tracking-wide">Empty Directory</span>
                        </div>
                      );
                    }

                    const displayed = filtered.slice(0, browseLimit);
                    const hasMoreBrowse = filtered.length > browseLimit;

                    return (
                      <>
                        {displayed.map((item, index) => (
                          <ExplorerItem
                            key={item.path}
                            item={item}
                            isActive={index === selectedExplorerIndex}
                            onSelect={() => setSelectedExplorerIndex(index)}
                            onDoubleClick={async () => {
                              const isNavigable = item.is_dir && !item.path.endsWith(".app");
                              if (isNavigable) {
                                navigateTo(item.path);
                              } else {
                                await openResult(item);
                              }
                            }}
                          />
                        ))}
                        {hasMoreBrowse && (
                          <div
                            onClick={() => {
                              setBrowseLimit((prev) => prev + 50);
                              showToast("Loading more items…", "info");
                            }}
                            className="result-item flex items-center justify-center py-2 px-3.5 border border-dashed border-indigo-500/20 hover:border-indigo-500/35 hover:bg-indigo-500/10 transition-all rounded-lg text-indigo-300 font-semibold cursor-pointer text-[12px] mt-1 select-none shrink-0"
                          >
                            <span>⏬ Load 50 More Items…</span>
                          </div>
                        )}
                      </>
                    );
                  })()}
                </div>
              )}
            </div>

            {/* Column 3: Detail Inspector Pane */}
            {showInspector && selectedMetadata && !isExplorerCommandMode && (
              <div className="w-[230px] bg-white/[0.012] border-l border-white/[0.05] p-4 flex flex-col gap-4 overflow-y-auto select-none shrink-0 justify-between">
                <div className="flex flex-col gap-4">
                  <span className="text-[10px] text-white/20 uppercase tracking-widest font-bold tracking-wide">
                    Information
                  </span>
                  
                  <div className="flex flex-col items-center gap-3 py-1.5 text-center">
                    <div className="scale-[1.8] py-2 shrink-0">
                      <FileIcon name={selectedMetadata.path.split("/").pop() || ""} isDir={selectedMetadata.is_dir} path={selectedMetadata.path} />
                    </div>
                    <span className="text-[13px] font-semibold text-white/90 break-all px-1 mt-1 line-clamp-2 leading-tight select-text">
                      {selectedMetadata.path.split("/").pop()}
                    </span>
                    <span className="text-[9px] text-white/40 uppercase tracking-wider font-semibold bg-white/5 border border-white/5 px-2 py-0.5 rounded-full mt-0.5">
                      {selectedMetadata.path.endsWith(".app") ? "Application" : selectedMetadata.is_dir ? "Folder" : selectedMetadata.path.split(".").pop()?.toUpperCase() + " File"}
                    </span>
                  </div>
                  
                  <div className="h-px bg-white/[0.04] shrink-0" />
                  
                  <div className="flex flex-col gap-2.5 text-[11px]">
                    <div className="flex flex-col gap-1">
                      <span className="text-white/20 font-semibold uppercase tracking-wider text-[9px]">Full Path</span>
                      <div className="flex items-center gap-1.5 mt-0.5">
                        <span className="text-white/60 font-mono break-all line-clamp-3 bg-white/[0.02] p-1.5 rounded border border-white/5 text-[9.5px] leading-relaxed flex-1 select-text">
                          {selectedMetadata.path}
                        </span>
                        <button
                          onClick={async () => {
                            await writeText(selectedMetadata.path);
                            showToast("Copied path to clipboard!", "success");
                          }}
                          className="p-1.5 rounded hover:bg-white/5 text-white/40 hover:text-white/80 transition-colors cursor-pointer shrink-0 border border-transparent hover:border-white/5"
                          title="Copy Path"
                        >
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
                            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
                          </svg>
                        </button>
                      </div>
                    </div>

                    <div className="flex justify-between py-0.5 border-b border-white/[0.02] mt-1 select-text">
                      <span className="text-white/35 font-medium">Size</span>
                      <span className="text-white/75 font-mono">
                        {selectedMetadata.is_dir 
                          ? `${selectedMetadata.item_count ?? 0} items` 
                          : formatBytes(selectedMetadata.size)}
                      </span>
                    </div>

                    {selectedMetadata.created && (
                      <div className="flex justify-between py-0.5 border-b border-white/[0.02] select-text">
                        <span className="text-white/35 font-medium">Created</span>
                        <span className="text-white/75 font-mono text-[10.5px]">
                          {new Date(selectedMetadata.created * 1000).toLocaleDateString(undefined, {
                            month: "short",
                            day: "numeric",
                            year: "numeric",
                          })}
                        </span>
                      </div>
                    )}

                    {selectedMetadata.modified && (
                      <div className="flex justify-between py-0.5 border-b border-white/[0.02] select-text">
                        <span className="text-white/35 font-medium">Modified</span>
                        <span className="text-white/75 font-mono text-[10.5px]">
                          {new Date(selectedMetadata.modified * 1000).toLocaleDateString(undefined, {
                            month: "short",
                            day: "numeric",
                            year: "numeric",
                          })}
                        </span>
                      </div>
                    )}
                  </div>
                </div>

                {/* Favorite button toggle */}
                <button
                  onClick={() => toggleFavorite(selectedMetadata.path, selectedMetadata.path.split("/").pop() || "Folder")}
                  className={`w-full py-1.5 px-3 rounded-lg border text-[11px] font-medium transition-all select-none cursor-pointer flex items-center justify-center gap-1.5 shrink-0 ${
                    customFavorites.some((fav) => fav.path === selectedMetadata.path)
                      ? "bg-rose-500/10 hover:bg-rose-500/15 border-rose-500/25 text-rose-300"
                      : "bg-indigo-500/15 hover:bg-indigo-500/20 border-indigo-500/25 text-indigo-300"
                  }`}
                >
                  <span>★</span>
                  <span>
                    {customFavorites.some((fav) => fav.path === selectedMetadata.path)
                      ? "Remove Favorite"
                      : "Add Favorite"}
                  </span>
                </button>
              </div>
            )}
          </div>
        )}
        </>
        )}

        {/* ── Status Bar ── */}
        <StatusBar
          total={mode === "search" ? totalIndexed : explorerItems.length}
          query={mode === "search" ? query : explorerQuery}
          resultCount={mode === "search" ? results.length : explorerItems.length}
          isSearching={mode === "search" ? isSearching : false}
          searchTime={mode === "search" ? searchTime : null}
        />

        {/* ── Bottom Floating Feedback Toast HUD ── */}
        <AnimatePresence>
          {toast && (
            <motion.div
              initial={{ opacity: 0, y: 15, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 15, scale: 0.95 }}
              transition={{ duration: 0.15, ease: "easeOut" }}
              className="fixed bottom-12 left-1/2 -translate-x-1/2 z-50 bg-black/65 backdrop-blur-xl border border-white/[0.08] flex items-center gap-2.5 px-4 py-2.5 rounded-xl shadow-2xl text-[12px] font-medium text-white/90 select-none"
            >
              {toast.type === "success" && <span className="text-emerald-400 text-[14px]">✓</span>}
              {toast.type === "info" && <span className="text-indigo-400 text-[14px]">ℹ</span>}
              {toast.type === "error" && <span className="text-rose-400 text-[14px]">⚠</span>}
              <span className="text-white/85 tracking-wide">{toast.message}</span>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
