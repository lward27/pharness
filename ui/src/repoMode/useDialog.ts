import { useEffect, useRef, type RefObject } from "react";

/** Keep focus lifecycle independent of parent polling/rerenders. */
export function useDialog(ref: RefObject<HTMLElement | null>, onClose: () => void) {
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const focusable = () => Array.from(ref.current?.querySelectorAll<HTMLElement>(
      'button:not(:disabled),input:not(:disabled),textarea:not(:disabled),select:not(:disabled),a[href],[tabindex="0"],summary',
    ) || []).filter(el => {
      if (el.closest('[hidden],[inert]')) return false;
      for (let ancestor: HTMLElement | null = el; ancestor && ancestor !== ref.current; ancestor = ancestor.parentElement) {
        const style = getComputedStyle(ancestor);
        if (style.display === "none" || style.visibility === "hidden") return false;
        if (ancestor.matches("details:not([open])") && !ancestor.querySelector(":scope > summary")?.contains(el)) return false;
      }
      return true;
    });
    const initial = focusable();
    (initial.find(el => el.matches("[autofocus], input")) || initial[0] || ref.current)?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); close.current(); }
      if (event.key !== "Tab") return;
      const nodes = focusable();
      const first = nodes[0], last = nodes[nodes.length - 1];
      if (!first) { event.preventDefault(); return; }
      if (event.shiftKey && (document.activeElement === first || !ref.current?.contains(document.activeElement))) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || !ref.current?.contains(document.activeElement))) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", keydown);
    return () => { document.removeEventListener("keydown", keydown); if (previous?.isConnected) previous.focus(); };
  }, [ref]);
}
