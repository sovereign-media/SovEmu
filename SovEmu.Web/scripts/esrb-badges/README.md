# ESRB rating icons

The ESRB's own rating icons (the 2013 set), as Wikimedia Commons hosts them:
`ESRB 2013 Early Childhood.svg`, `Everyone`, `Everyone 10+`, `Teen`, `Mature`,
`Adults Only 18+` and `Rating Pending` (https://commons.wikimedia.org/wiki/File:ESRB_2013_Teen.svg
and its neighbours). Commons lists them as public domain (simple text logos); they are
trademarks of the Entertainment Software Association, shown here only to say what a game is
rated.

`render.mjs` (`npm run esrb-badges`) renders them to the WebP images every client ships: this
web client's `src/assets/esrb/`, and copies in the Android (`res/drawable-nodpi/`) and iOS
(`Apps/Mobile/Esrb/`) apps.
