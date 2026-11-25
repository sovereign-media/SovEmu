# ESRB rating icons

The ESRB's own rating icons (the 2013 set), as Wikimedia Commons hosts them:
`ESRB 2013 Early Childhood.svg`, `Everyone`, `Everyone 10+`, `Teen`, `Mature`,
`Adults Only 18+` and `Rating Pending` (https://commons.wikimedia.org/wiki/File:ESRB_2013_Teen.svg
and its neighbours). Commons lists them as public domain (simple text logos); they are
trademarks of the Entertainment Software Association, shown here only to say what a game is
rated.

`render.mjs` (`npm run esrb-badges`) renders them to the PNGs the server ships
(`SovEmu.Backend/SovEmu.Library/assets/ratings/esrb/`), written into its built-in artwork at
`artwork/ratings/esrb/<rating>.png` (`/opt/sovemu/artwork` in the image) and served to every
client. An admin replaces one by putting an image at the same path in `<data>/artwork/`.
