
// File extension → color + icon character mapping
const EXT_MAP: Record<string, { bg: string; color: string; label: string }> = {
  // Rust
  rs:   { bg: "rgba(247,76,0,0.15)",   color: "#f74c00", label: "RS"  },
  // JavaScript / TypeScript
  js:   { bg: "rgba(247,220,0,0.12)",  color: "#f7dc00", label: "JS"  },
  ts:   { bg: "rgba(49,120,198,0.18)", color: "#3178c6", label: "TS"  },
  tsx:  { bg: "rgba(49,120,198,0.18)", color: "#61dafb", label: "TSX" },
  jsx:  { bg: "rgba(247,220,0,0.12)",  color: "#f7dc00", label: "JSX" },
  // Web
  html: { bg: "rgba(228,77,38,0.15)",  color: "#e44d26", label: "HTM" },
  css:  { bg: "rgba(38,77,228,0.15)",  color: "#264de4", label: "CSS" },
  // Config
  json: { bg: "rgba(200,200,200,0.1)", color: "#a8a8a8", label: "JSON"},
  toml: { bg: "rgba(200,200,200,0.1)", color: "#a8a8a8", label: "TOML"},
  yaml: { bg: "rgba(200,200,200,0.1)", color: "#a8a8a8", label: "YML" },
  yml:  { bg: "rgba(200,200,200,0.1)", color: "#a8a8a8", label: "YML" },
  // Docs
  md:   { bg: "rgba(255,255,255,0.07)", color: "#e2e2e2", label: "MD" },
  txt:  { bg: "rgba(255,255,255,0.07)", color: "#e2e2e2", label: "TXT"},
  // Images
  png:  { bg: "rgba(168,85,247,0.15)", color: "#a855f7", label: "PNG" },
  jpg:  { bg: "rgba(168,85,247,0.15)", color: "#a855f7", label: "JPG" },
  svg:  { bg: "rgba(168,85,247,0.15)", color: "#a855f7", label: "SVG" },
  // Python
  py:   { bg: "rgba(55,118,171,0.18)", color: "#3776ab", label: "PY"  },
  // Shell
  sh:   { bg: "rgba(30,215,96,0.12)",  color: "#1ed760", label: "SH"  },
  zsh:  { bg: "rgba(30,215,96,0.12)",  color: "#1ed760", label: "ZSH" },
  // Go
  go:   { bg: "rgba(0,173,216,0.15)",  color: "#00add8", label: "GO"  },
};

const DEFAULT_FILE = { bg: "rgba(255,255,255,0.07)", color: "#a0a0a0", label: "FILE" };
const DIR_STYLE    = { bg: "rgba(99,102,241,0.18)",  color: "#818cf8" };

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";

// Keep a simple global in-memory cache of already resolved base64 icons
// so that mounting a file list doesn't trigger redundant IPC calls for the same apps!
const globalIconCache: Record<string, string> = {};

interface FileIconProps {
  name: string;
  isDir: boolean;
  path?: string;
}

export function FileIcon({ name, isDir, path }: FileIconProps) {
  const isApp = name.endsWith(".app") || path?.endsWith(".app");
  const [iconSrc, setIconSrc] = useState<string | null>(() => {
    if (isApp && path) {
      return globalIconCache[path] || null;
    }
    return null;
  });

  useEffect(() => {
    if (!isApp || !path || iconSrc) return;

    let isMounted = true;
    invoke<string>("get_app_icon", { path })
      .then((base64) => {
        if (isMounted) {
          globalIconCache[path] = base64;
          setIconSrc(base64);
        }
      })
      .catch((err) => {
        console.error("Failed to load app icon:", err);
      });

    return () => {
      isMounted = false;
    };
  }, [isApp, path, iconSrc]);

  if (isApp) {
    if (iconSrc) {
      return (
        <div className="file-icon-wrapper p-0 bg-transparent flex items-center justify-center overflow-hidden w-[24px] h-[24px] shrink-0">
          <img
            src={iconSrc}
            alt={name}
            className="w-[20px] h-[20px] object-contain select-none pointer-events-none"
          />
        </div>
      );
    }
    return (
      <div
        className="file-icon-wrapper bg-rose-500/15 flex items-center justify-center w-[24px] h-[24px] shrink-0"
        style={{ color: "#fb7185" }}
      >
        <span className="text-[12px] animate-pulse">🚀</span>
      </div>
    );
  }

  if (isDir) {
    return (
      <div
        className="file-icon-wrapper"
        style={{ background: DIR_STYLE.bg }}
      >
        {/* Folder SVG */}
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none">
          <path
            d="M3 7C3 5.9 3.9 5 5 5H9.586C9.851 5 10.105 5.105 10.293 5.293L11.707 6.707C11.895 6.895 12.149 7 12.414 7H19C20.1 7 21 7.9 21 9V18C21 19.1 20.1 20 19 20H5C3.9 20 3 19.1 3 18V7Z"
            fill={DIR_STYLE.color}
            fillOpacity="0.9"
          />
        </svg>
      </div>
    );
  }

  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const meta = EXT_MAP[ext] ?? DEFAULT_FILE;

  return (
    <div
      className="file-icon-wrapper"
      style={{ background: meta.bg }}
    >
      <span
        className="text-[9px] font-bold tracking-wider"
        style={{ color: meta.color }}
      >
        {meta.label}
      </span>
    </div>
  );
}
