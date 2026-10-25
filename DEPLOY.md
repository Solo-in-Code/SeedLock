# Publishing seedlock.no on GitHub Pages

## 1. Put the files on GitHub
1. On GitHub, create a new **public** repository, for example `seedlock-website` (in the Solo-in-Code account).
2. Upload everything in this folder (including `CNAME`, `.nojekyll`, `assets/`) to the repository root, on the `main` branch.
   (`.nojekyll` and `CNAME` start with a dot or have no extension, so check that they uploaded.)

## 2. Turn on Pages
Repository > Settings > Pages:
- Source: **Deploy from a branch**, branch `main`, folder `/ (root)`.
- Custom domain: `seedlock.no` > Save.
- When the DNS check passes, tick **Enforce HTTPS**.

## 3. DNS at your registrar (for seedlock.no)
Remove other A/AAAA/CNAME records for `@` and `www`, then add:

| Type  | Name | Value |
|-------|------|-------|
| A     | @    | 185.199.108.153 |
| A     | @    | 185.199.109.153 |
| A     | @    | 185.199.110.153 |
| A     | @    | 185.199.111.153 |
| AAAA  | @    | 2606:50c0:8000::153 |
| AAAA  | @    | 2606:50c0:8001::153 |
| AAAA  | @    | 2606:50c0:8002::153 |
| AAAA  | @    | 2606:50c0:8003::153 |
| CNAME | www  | solo-in-code.github.io |

DNS can take from a few minutes to a day. Check these values against
https://docs.github.com/pages/configuring-a-custom-domain-for-your-github-pages-site
before you use them, in case GitHub has changed them.

## 4. Security checklist
- Turn on two-factor login on GitHub and at the domain registrar.
- Verify the domain on GitHub (Settings > Pages > Add a domain, or your account's Pages verification) so nobody else can claim it.
- Keep the SHA-256 list and key fingerprint on GitHub too, so people can compare in two places.

## 5. Updating
Change a file, commit, push. The site updates in about a minute.
The Download page reads the newest GitHub release by itself.
Only the Start page has the version number (0.3.0) written by hand.
