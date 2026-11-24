// A screen's side panel, open on a game named in its URL (`?game=`): opening it pushes a history
// entry, so the browser's Back closes it, and coming back to the screen (Back from a game) finds
// it open again.

import { useEffect, useRef, useState } from "react";
import { gameFromUrl, urlWithGame } from "../lib/games";

export function useGameInUrl(): [openId: number | null, open: (id: number) => void, close: () => void] {
  const [openId, setOpenId] = useState<number | null>(() => gameFromUrl(location.search));
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
      history.pushState(null, "", url);
      pushed.current = true;
    } else {
      history.replaceState(null, "", url);
    }
    setOpenId(id);
  };
  const close = () => {
    if (gameFromUrl(location.search) === null) return setOpenId(null);
    if (pushed.current) {
      history.back(); // popstate closes it
    } else {
      history.replaceState(null, "", urlWithGame(location.href, null));
      setOpenId(null);
    }
  };
  return [openId, open, close];
}
