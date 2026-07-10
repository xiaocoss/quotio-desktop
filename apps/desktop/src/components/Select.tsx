import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export type SelectOption = { value: string; label: string };

type SelectProps = {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  /** Minimum width of the trigger button, e.g. "132px". */
  minWidth?: string;
};

/** 展开时菜单在视口里的固定坐标(右对齐按钮右边缘)。 */
type MenuPos = { top: number; right: number; minWidth: number };

/// A custom dropdown whose popup is fully styled (unlike a native <select>,
/// whose option list is OS-rendered and can't be themed).
///
/// 菜单**必须 portal 到 `document.body` 并用 `position: fixed`**,不能就地
/// `position: absolute`:`.panel` 带 `backdrop-filter`,而 `backdrop-filter` 会建立
/// **层叠上下文**,把菜单的 `z-index` 困在面板内部 —— 面板本身是 static/z-auto,于是
/// 排在它后面的卡片、面板(它们同样因 backdrop-filter 建了上下文)一律画在菜单之上,
/// 表现为「下拉菜单被下方的 KPI 卡片切掉半截」。挂到 body 才能真正浮在最上层。
export function Select({ value, options, onChange, disabled = false, minWidth }: SelectProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<MenuPos | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const current = options.find((option) => option.value === value);

  const reposition = useCallback(() => {
    const button = rootRef.current?.querySelector("button");
    if (!button) return;
    const rect = button.getBoundingClientRect();
    setPos({
      top: rect.bottom + 5,
      right: Math.max(0, window.innerWidth - rect.right),
      minWidth: rect.width,
    });
  }, []);

  // 展开时先量好位置再绘制,避免菜单闪一下再归位。
  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    reposition();
  }, [open, reposition]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      // 菜单在 portal 里,不再是 rootRef 的后代 —— 必须单独判定,否则 mousedown 会先
      // 关闭菜单、选项的 click 落空,点了等于没点。
      if (rootRef.current?.contains(target) || menuRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    // fixed 定位不会跟着祖先滚动容器走,滚动/缩放时要重新贴到按钮下方。
    // scroll 不冒泡,用捕获才能收到内部滚动容器(如仪表盘)的滚动。
    const onScrollOrResize = () => reposition();
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    document.addEventListener("scroll", onScrollOrResize, true);
    window.addEventListener("resize", onScrollOrResize);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("scroll", onScrollOrResize, true);
      window.removeEventListener("resize", onScrollOrResize);
    };
  }, [open, reposition]);

  const menu =
    open && pos
      ? createPortal(
          <div
            ref={menuRef}
            className="qselect-menu"
            role="listbox"
            style={{ top: pos.top, right: pos.right, minWidth: pos.minWidth }}
          >
            {options.map((option) => {
              const active = option.value === value;
              return (
                <button
                  key={option.value}
                  type="button"
                  role="option"
                  aria-selected={active}
                  className={active ? "qselect-option qselect-option--active" : "qselect-option"}
                  onClick={() => {
                    onChange(option.value);
                    setOpen(false);
                  }}
                >
                  <span>{option.label}</span>
                  {active ? (
                    <svg className="qselect-check" viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                      <path d="M3.5 8.5l3 3 6-6.5" />
                    </svg>
                  ) : null}
                </button>
              );
            })}
          </div>,
          document.body,
        )
      : null;

  return (
    <div className="qselect" ref={rootRef}>
      <button
        type="button"
        className="qselect-button"
        style={minWidth ? { minWidth } : undefined}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="qselect-value">{current?.label ?? value}</span>
        <svg className="qselect-chevron" viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M4 6l4 4 4-4" />
        </svg>
      </button>
      {menu}
    </div>
  );
}
