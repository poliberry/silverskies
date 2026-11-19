# Wiki source

Every Markdown file here (except this README) is a page of the
[GitHub Wiki](https://github.com/poliberry/silverskies/wiki). The **Wiki** workflow
(`.github/workflows/wiki.yml`) copies this folder to the wiki on every push to `main`
that touches it, so edit the pages here, by pull request, not in the wiki itself
(changes made there are overwritten).

- File names become page names (`Silver-Skies-Pro.md` → "Silver Skies Pro").
- Link between pages with `[text](Page-Name)` (no `.md`).
- `_Sidebar.md` and `_Footer.md` appear on every page.

One-time setup: the wiki must be switched on (repository **Settings → General →
Features → Wikis**) and have at least one page (create any page in the browser once);
the workflow then replaces it with these pages.
