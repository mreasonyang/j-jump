# J-Jump website

The official bilingual website targets [j-jump.kyun.si](https://j-jump.kyun.si/).
It uses plain HTML, CSS, and JavaScript. Node.js 20 or later is the only build requirement;
there are no npm dependencies to install. The terminal demonstration is illustrative.

## Build and preview

From the repository root:

```sh
cd website
npm run check
npm run build
npm run preview
```

Open [the local preview](http://127.0.0.1:4187/). The server binds to `0.0.0.0:4187`
for access from other devices and serves only the generated `dist` folder. Stop it with
Ctrl-C. Rebuild after changing files in `public`.

`check` validates JavaScript syntax, local asset references, internal anchors, canonical
metadata, and sitemap configuration. It rejects unsupported file types, symlinks, hidden
entries, private folders, and machine-specific paths in the published assets. It does not
replace browser checks of English and Chinese content, copy buttons, terminal demo,
responsive layout, keyboard navigation, or external links.

Published files are limited to web pages, styles, JavaScript, SVG/XML/text, common image
formats, and WOFF2 fonts. The root-level Cloudflare `_headers` and `_redirects` files are
also supported and checked as text. The preview server serves static files; Cloudflare
applies the hosting configuration during deployment.

## Package for Cloudflare Pages

```sh
cd website
npm run package
```

This validates and rebuilds the site, then writes `j-jump-site.zip` and prints its SHA-256.
The archive is deterministic and places `index.html` at its root. It contains only files
copied from `public`; repository source, records, build tools, and credentials are excluded.
The `dist` directory and ZIP are generated outputs and should not be committed.

In the Cloudflare dashboard:

1. Open **Workers & Pages**, choose **Create application**, then the **Pages** direct-upload flow.
2. Create the `j-jump` Pages project and upload `j-jump-site.zip` (or the contents of `dist`).
3. Deploy the site and verify the project’s `pages.dev` URL.
4. In the Pages project’s **Custom domains**, add `j-jump.kyun.si` and complete Cloudflare’s
   DNS and certificate setup for the `kyun.si` zone.
5. Verify the HTTPS homepage, CSS, JavaScript, favicon, missing-page behavior, and both languages.

For subsequent releases, create a new production deployment in the same Pages project
and upload the newly generated ZIP. Keep its hash with the deployment evidence. A rollback
uses the preceding deployment in the Pages dashboard. Website deployment does not publish
a CLI release or trigger the repository’s guarded release workflow.

Cloudflare Direct Upload projects cannot later switch to Git integration; automatic Git
deployments require a separate project. See the official [Direct Upload documentation](https://developers.cloudflare.com/pages/get-started/direct-upload/)
and [custom domain setup](https://developers.cloudflare.com/pages/configuration/custom-domains/).

## Content changes

Edit `public/index.html`, `public/styles.css`, and `public/app.js`. Keep installation
commands and product claims aligned with the repository documentation. Update both language
dictionaries together. Retain the canonical origin `https://j-jump.kyun.si` in HTML metadata,
`robots.txt`, and `sitemap.xml`.
