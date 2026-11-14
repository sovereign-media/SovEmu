// Short notices ("Saved to slot 2") on Radix Toast: `useToast()(text)` from anywhere under
// <Toasts>.
import { Toast } from "radix-ui";
import { createContext, useCallback, useContext, useState, type ReactNode } from "react";

const Context = createContext<(text: string) => void>(() => {});

export const useToast = () => useContext(Context);

export function Toasts({ children }: { children: ReactNode }) {
  const [notes, setNotes] = useState<{ id: number; text: string }[]>([]);
  const show = useCallback((text: string) => setNotes((n) => [...n.slice(-2), { id: Date.now() + Math.random(), text }]), []);
  return (
    <Context.Provider value={show}>
      <Toast.Provider duration={2500} swipeDirection="down">
        {children}
        {notes.map((n) => (
          <Toast.Root
            key={n.id}
            onOpenChange={(open) => !open && setNotes((all) => all.filter((x) => x.id !== n.id))}
            className="rounded-full bg-black/85 px-4 py-2 text-sm text-slate-100 shadow-xl"
          >
            <Toast.Description>{n.text}</Toast.Description>
          </Toast.Root>
        ))}
        <Toast.Viewport className="pointer-events-none fixed bottom-6 left-1/2 z-[60] flex -translate-x-1/2 flex-col items-center gap-2 outline-none" />
      </Toast.Provider>
    </Context.Provider>
  );
}
