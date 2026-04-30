import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { motion } from "framer-motion";

export interface ContextMenuItem {
  label: string;
  shortcut?: string;
  onClick: () => void;
  disabled?: boolean;
  separator?: false;
}

export interface ContextMenuSeparator {
  separator: true;
}

export type ContextMenuEntry = ContextMenuItem | ContextMenuSeparator;

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuEntry[];
  onClose: () => void;
}

const EDGE_PADDING = 8;

export default function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  const [maxHeight, setMaxHeight] = useState<number | undefined>(undefined);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    setTimeout(() => {
      document.addEventListener("mousedown", handleClickOutside);
      document.addEventListener("keydown", handleEscape);
    }, 0);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [onClose]);

  // Position the menu so it always fits inside the viewport. Use offsetWidth/
  // offsetHeight (layout dimensions, ignores transforms) so framer-motion's
  // scale animation doesn't throw off measurement.
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    let left = x;
    let top = y;

    // Horizontal: if it would overflow right, anchor to the right edge instead
    if (left + w > vw - EDGE_PADDING) {
      left = Math.max(EDGE_PADDING, vw - w - EDGE_PADDING);
    }
    if (left < EDGE_PADDING) left = EDGE_PADDING;

    // Vertical: if menu can fit above the click point, flip upward; otherwise
    // anchor to the bottom edge with internal scrolling
    const spaceBelow = vh - y - EDGE_PADDING;
    const spaceAbove = y - EDGE_PADDING;
    let cap: number | undefined;
    if (h <= spaceBelow) {
      top = y;
    } else if (h <= spaceAbove) {
      // Flip up
      top = y - h;
    } else {
      // Doesn't fit either way: cap height and place against the larger side
      if (spaceBelow >= spaceAbove) {
        top = y;
        cap = spaceBelow;
      } else {
        top = EDGE_PADDING;
        cap = spaceAbove;
      }
    }

    setPos({ left, top });
    setMaxHeight(cap);
  }, [x, y, items]);

  return (
    <motion.div
      ref={menuRef}
      initial={{ opacity: 0, scale: 0.92 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.1, ease: "easeOut" }}
      className="glass-elevated fixed z-[110] min-w-[200px] rounded-xl py-1.5 shadow-2xl"
      style={{
        left: pos.left,
        top: pos.top,
        maxHeight: maxHeight ? `${maxHeight}px` : undefined,
        overflowY: maxHeight ? "auto" : undefined,
      }}
    >
      {items.map((item, i) => {
        if (item.separator) {
          return <div key={i} className="mx-2 my-1 border-t border-white/6" />;
        }
        return (
          <button
            key={i}
            disabled={item.disabled}
            onClick={() => { item.onClick(); onClose(); }}
            className="flex w-full items-center justify-between px-3 py-1.5 text-left text-[12px] text-white/70 transition-colors hover:bg-white/8 hover:text-white disabled:text-white/20 disabled:hover:bg-transparent"
          >
            <span>{item.label}</span>
            {item.shortcut && (
              <span className="ml-8 text-[10px] text-white/20 font-medium">{item.shortcut}</span>
            )}
          </button>
        );
      })}
    </motion.div>
  );
}
