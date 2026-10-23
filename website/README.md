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
metadata, social-card tags and image dimensions, and sitemap configuration. It rejects
unsupported file types, symlinks, hidden entries, private folders, and machine-specific
paths in the published assets. It does not
replace browser checks of English and Chinese content, copy buttons, terminal demo,
responsive layout, keyboard navigation, or external links.

Published files are limited to web pages, styles, JavaScript, SVG/XML/text, common image
formats, and WOFF2 fonts. The root-level Cloudflare `_headers` and `_redirects` files are
also supported and checked as text. The preview server serves static files; Cloudflare
applies the hosting configuration during deployment.

## Deploy with Cloudflare Pages and GitHub

Cloudflare Pages builds the website directly from the
[`mreasonyang/j-jump` GitHub repository](https://github.com/mreasonyang/j-jump).
Use the native Cloudflare GitHub App connection and the following project settings:

| Setting | Value |
| --- | --- |
| Pages project | `j-jump-site` |
| Production branch | `main` |
| Framework preset | None |
| Root directory | `website` |
| Build command | `npm run build` |
| Build output directory | `dist` |
| Environment variable | `NODE_VERSION=22` |

The build validates the public assets before copying them to `dist`. It requires no npm
dependencies or Cloudflare API token. Only the generated website assets are published;
repository source, private records, build tools, and credentials are excluded.

In the Cloudflare dashboard:

1. Open **Workers & Pages**, choose **Create application**, then the **Pages** Git integration flow.
2. Connect GitHub and authorize the Cloudflare GitHub App for `mreasonyang/j-jump`.
   Preserve any repositories already authorized for other sites.
3. Create `j-jump-site` with the settings above. Keep automatic production deployments
   enabled for `main`.
4. Wait for the Git build and deployment to succeed, check its source commit, and verify
   `https://j-jump-site.pages.dev/` before moving the custom domain.
5. Move `j-jump.kyun.si` from the existing direct-upload project `j-jump` to `j-jump-site`
   through **Custom domains** and complete the DNS and certificate setup for `kyun.si`.
6. Verify the HTTPS homepage, CSS, JavaScript, favicon, missing-page behavior, and both
   languages. Retain the old `j-jump` project and its deployment as a rollback baseline.

After setup, an ordinary push to `main` triggers Cloudflare's build and production deployment.
Check the deployment status and source commit in Pages; no file upload is needed. Use a
previous successful deployment in the Pages dashboard for a routine rollback. During the
migration, the retained `j-jump` project also permits restoring the previous domain binding.
Website deployment does not publish a CLI release or trigger the repository's guarded
release workflow.

Cloudflare Direct Upload projects cannot later switch to Git integration, which is why
the Git-connected site uses a separate project. See the official
[Git integration documentation](https://developers.cloudflare.com/pages/get-started/git-integration/),
[Direct Upload documentation](https://developers.cloudflare.com/pages/get-started/direct-upload/)
and [custom domain setup](https://developers.cloudflare.com/pages/configuration/custom-domains/).

## Optional offline package

`npm run package` validates and rebuilds the site, writes a deterministic `j-jump-site.zip`,
and prints its SHA-256. The archive places `index.html` at its root and contains only files
copied from `public`. This is an optional offline artifact; production deployment uses the
Git integration above. The generated `dist` directory and ZIP should not be committed.

## Content changes

Edit `public/index.html`, `public/styles.css`, and `public/app.js`. Keep installation
commands and product claims aligned with the repository documentation. Update both language
dictionaries together. Retain the canonical origin `https://j-jump.kyun.si` in HTML metadata,
`robots.txt`, and `sitemap.xml`.

## Social sharing

The root page includes static Open Graph and Twitter/X `summary_large_image` metadata.
Both languages use the same root URL, so crawlers receive the default English card
without needing JavaScript. `public/social-card.png` is the 1200×630 public share image;
its editable artwork is `social-card.svg` outside the published folder. Render it with
a standards-compliant SVG rasterizer (for example Sharp/librsvg) when updating the artwork.
The normal build uses the committed PNG and needs no image-rendering dependency.

`npm run check` requires unique, matching title/description/image/alt tags, an absolute
HTTPS image URL, the matching PNG type and dimensions, and an image below 5 MB. After
deployment, check the root and image with a `Twitterbot/1.0` user agent and compare their
bodies with the build. X may retain an earlier card until its crawler refreshes the URL;
HTTP and metadata checks do not establish what an existing X post currently displays.
