// Before any unit test runs: Node builds its Intl data (locales, date and number formats) on first
// use, which on a loaded CI runner took long enough to time out whichever test formatted a date or
// a number first (5 s, in tests that take a millisecond). Doing it here keeps that out of every
// test's own time.

new Intl.DateTimeFormat("en-GB", { dateStyle: "medium", timeStyle: "short" }).format(0);
new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short", year: "numeric" }).format(0);
new Date(0).toLocaleString();
(1234.5).toLocaleString("en-US");
