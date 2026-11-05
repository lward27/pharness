// The terminal widget behind a small interface, so the page can be tested
// without a canvas and the widget swapped without touching the page.
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

export type TerminalWidget = {
  open: (element: HTMLElement) => void;
  write: (bytes: Uint8Array | string) => void;
  onData: (listener: (data: string) => void) => void;
  onResize: (listener: (size: { cols: number; rows: number }) => void) => void;
  /** Return false from the handler to keep a key from the terminal. */
  onKey: (handler: (event: KeyboardEvent) => boolean) => void;
  fit: () => void;
  focus: () => void;
  reset: () => void;
  setScreenReaderMode: (enabled: boolean) => void;
  size: () => { cols: number; rows: number };
  dispose: () => void;
};

export function createXtermWidget(): TerminalWidget {
  const terminal = new Terminal({
    cursorBlink: true,
    convertEol: false,
    fontFamily: '"IBM Plex Mono", ui-monospace, monospace',
    fontSize: 14,
    scrollback: 5000,
    theme: { background: "#0b1020", foreground: "#e6e9f2", cursor: "#9ecbff" },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  return {
    open: element => terminal.open(element),
    write: bytes => terminal.write(bytes),
    onData: listener => {
      terminal.onData(listener);
    },
    onResize: listener => {
      terminal.onResize(listener);
    },
    onKey: handler => terminal.attachCustomKeyEventHandler(handler),
    fit: () => {
      try {
        fit.fit();
      } catch {
        // Not laid out yet; the next resize fits it.
      }
    },
    focus: () => terminal.focus(),
    reset: () => terminal.reset(),
    setScreenReaderMode: enabled => {
      terminal.options.screenReaderMode = enabled;
    },
    size: () => ({ cols: terminal.cols, rows: terminal.rows }),
    dispose: () => terminal.dispose(),
  };
}
