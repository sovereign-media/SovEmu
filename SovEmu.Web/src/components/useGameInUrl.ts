// A screen's side panel, open on a game named in its URL (`?game=`): opening it pushes a history
// entry, so the browser's Back closes it, and coming back to the screen (Back from a game) finds
// it open again.
//
// The URL changes through the router with `resetScroll: false`: the router sees every history
// change, and would otherwise take this one for a new page and scroll the grid back to the top.

import { useRouter } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import { gameFromUrl, urlWithGame } from "../lib/games";

export function useGameInUrl(): [openId: number | null, open: (id: number) => void, close: () => void] {
  const router = useRouter();
  const [openId, setOpenId] = useState<number | null>(() => gameFromUrl(location.search));
  /** Changes the URL where the screen stays as it is, scrolled where it was. */
  const go = (href: string, replace: boolean) => router.navigate({ href, replace, resetScroll: false });
  /** Whether this screen pushed the panel's history entry (so closing it goes Back). */
  const pushed = useRef(false);

  useEffect(() => {
    const onPop = () => {
      pushed.current = false;
      setOpenId(gameFromUrl(location.search));
    };
    addEventListener("popstate", onPop);
    return () => removeEventListener("popstate", onPop);
  }, []);

  /** Opens the panel on a game: a new history entry, or the same one when it is already open. */
  const open = (id: number) => {
    const url = urlWithGame(location.href, id);
    if (gameFromUrl(location.search) === null) {
      go(url, false);
      pushed.current = true;
    } else {
      go(url, true);
    }
    setOpenId(id);
  };
  const close = () => {
    if (gameFromUrl(location.search) === null) return setOpenId(null);
    if (pushed.current) {
      history.back(); // popstate closes it
    } else {
      go(urlWithGame(location.href, null), true);
      setOpenId(null);
    }
  };
  return [openId, open, close];
}
