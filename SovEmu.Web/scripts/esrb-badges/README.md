# ESRB rating icons

The ESRB's own rating icons (the 2013 set), as Wikimedia Commons hosts them:
`ESRB 2013 Early Childhood.svg`, `Everyone`, `Everyone 10+`, `Teen`, `Mature`,
`Adults Only 18+` and `Rating Pending` (https://commons.wikimedia.org/wiki/File:ESRB_2013_Teen.svg
and its neighbours). Commons lists them as public domain (simple text logos); they are
trademarks of the Entertainment Software Association, shown here only to say what a game is
rated.

`render.mjs` (`npm run esrb-badges`) renders them to the PNGs the server ships in its built-in
decoration set (`SovEmu.Backend/SovEmu.Library/assets/ratings/esrb/`), served to every client
from `<decorations>/sovemu/ratings/esrb/<rating>.png`. An admin replaces one by putting an image
at that path in `<data>/decorations/sovemu/` or a `SOVEMU_DECORATIONS_DIRS` folder.
