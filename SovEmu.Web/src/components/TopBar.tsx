// The top bar: wordmark, an optional crumb, admin links, the signed-in user's menu and a clock.
import { Clock, Gamepad2, Library, LogOut, Settings2, UserRound } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type User } from "../lib/api";
import { Button } from "./ui/button";
import { Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger } from "./ui/primitives";

const time = () => new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

export default function TopBar({ crumb }: { crumb?: string }) {
  const [user, setUser] = useState<User | null>(null);
  const [clock, setClock] = useState(time);
  // Over a scrolled page the bar needs a backdrop of its own; at the top it floats on the hero.
  const [scrolled, setScrolled] = useState(false);

  useEffect(() => {
    api.status().then((s) => setUser(s.user));
    const t = setInterval(() => setClock(time()), 10_000);
    const onScroll = () => setScrolled(scrollY > 24);
    addEventListener("scroll", onScroll, { passive: true });
    return () => {
      clearInterval(t);
      removeEventListener("scroll", onScroll);
    };
  }, []);

  const signOut = async () => {
    await api.logout();
    location.href = "/login/";
  };

  return (
    <header
      className={`fixed inset-x-0 top-0 z-30 flex items-center gap-4 px-8 transition-[background,padding] duration-300 ${
        scrolled ? "bg-surface/90 py-3 shadow-lg shadow-black/40 backdrop-blur" : "bg-gradient-to-b from-black/70 to-transparent pt-5 pb-8"
      }`}
    >
      <a href="/" className="flex items-center gap-2 text-lg font-black tracking-tight text-white">
        <Gamepad2 className="size-5 text-accent" aria-hidden />
        <span>
          Sov<span className="text-accent">Emu</span>
        </span>
      </a>
      {crumb && <span className="text-sm tracking-[0.2em] text-slate-400 uppercase">{crumb}</span>}
      <div className="ml-auto flex items-center gap-2">
        {user?.is_admin && (
          <nav className="flex gap-1">
            <Button asChild variant="ghost" size="sm">
              <a href="/admin/">
                <Library /> Library
              </a>
            </Button>
            <Button asChild variant="ghost" size="sm">
              <a href="/admin/settings/">
                <Settings2 /> Settings
              </a>
            </Button>
          </nav>
        )}
        {user && (
          <Menu>
            <MenuTrigger asChild>
              <Button variant="ghost" size="sm" aria-label="Account">
                <UserRound /> {user.username}
              </Button>
            </MenuTrigger>
            <MenuContent>
              <MenuLabel>Signed in as {user.username}</MenuLabel>
              <MenuSeparator />
              <MenuItem onSelect={signOut}>
                <LogOut /> Sign out
              </MenuItem>
            </MenuContent>
          </Menu>
        )}
        <span className="ml-2 flex items-center gap-1.5 font-mono text-sm text-slate-300 tabular-nums">
          <Clock className="size-3.5 text-slate-500" aria-hidden />
          {clock}
        </span>
      </div>
    </header>
  );
}
