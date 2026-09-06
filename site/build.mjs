#!/usr/bin/env node
// Builds the deployable dalo.sh tree.
//
//   node build.mjs           render docs/*.md into site/docs/, stamp the
//                            version into site/index.html, assemble site/build/
//   node build.mjs --check   verify the checked-in output is up to date
//
// The rendered documentation and the stamped version are committed so the site
// stays deployable from a plain checkout; --check keeps them honest.

import { cp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { Marked } from "marked"

const siteDir = path.dirname(fileURLToPath(import.meta.url))
const rootDir = path.resolve(siteDir, "..")
const docsSourceDir = path.join(rootDir, "docs")
const docsOutDir = path.join(siteDir, "docs")
const buildDir = path.join(siteDir, "build")

const REPO = "https://github.com/sebastian-software/dalo"
const BLOB = `${REPO}/blob/main`
const SITE = "https://dalo.sh"

// Documentation pages published on dalo.sh, in reading order. The title comes
// from each document's own first heading; only navigation label and summary
// live here.
const PAGES = [
  {
    slug: "getting-started",
    label: "Getting started",
    summary: "Install Dalo, link an agent, add sources, and reach a first synced skill set.",
  },
  {
    slug: "reference",
    label: "Command reference",
    summary: "Every command, flag, config file, JSON report, and diagnostic code.",
  },
  {
    slug: "agents",
    label: "Agent integration",
    summary: "Supported agents, their skill directories, and how instruction packs are written.",
  },
  { slug: "ci", label: "Dalo in CI", summary: "Run a reproducible, non-interactive sync in a pipeline." },
  {
    slug: "troubleshooting",
    label: "Troubleshooting",
    summary: "Resolver, doctor, and security findings with the command that clears each one.",
  },
  { slug: "uninstall", label: "Uninstall", summary: "Remove targets, autosync, the store, and the binary." },
]

// Files that belong to the build step itself and are never deployed.
const NOT_DEPLOYED = new Set(["build", "build.mjs", "node_modules", "package.json", "pnpm-lock.yaml", "README.md"])

const escapeHtml = (value) =>
  value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;")

// GitHub-compatible heading slugs, so anchors written against the Markdown
// sources keep working on the rendered pages.
const slug = (text) =>
  text
    .trim()
    .toLowerCase()
    .replace(/[^\w\- ]+/g, "")
    .replace(/\s+/g, "-")

const uniqueSlug = (text, seen) => {
  const base = slug(text)
  const count = seen.get(base) ?? 0
  seen.set(base, count + 1)
  return count === 0 ? base : `${base}-${count}`
}

// Links are written for the repository; rewrite them for the site.
const rewriteLink = (href) => {
  if (/^(https?:|mailto:|#)/.test(href)) return href
  const [target, anchor = ""] = href.split("#")
  const suffix = anchor ? `#${anchor}` : ""
  if (/^[\w.-]+\.md$/.test(target)) return `${target.replace(/\.md$/, ".html")}${suffix}`
  if (target === "../site/install.md") return `/install.md${suffix}`
  if (target === "") return href
  return `${BLOB}/${target.replace(/^\.\.\//, "")}${suffix}`
}

const renderMarkdown = (markdown) => {
  const seen = new Map()
  const marked = new Marked({ gfm: true })
  marked.use({
    renderer: {
      heading(token) {
        const content = this.parser.parseInline(token.tokens)
        // The slug comes from the raw heading text, so anchors written against
        // the Markdown sources resolve on the rendered page too.
        const id = uniqueSlug(token.text, seen)
        const inner =
          token.depth === 1 ? content : `<a class="doc-anchor" href="#${id}">${content}</a>`
        return `<h${token.depth} id="${id}">${inner}</h${token.depth}>\n`
      },
      link(token) {
        const text = this.parser.parseInline(token.tokens)
        const target = rewriteLink(token.href)
        const titleAttr = token.title ? ` title="${escapeHtml(token.title)}"` : ""
        const external = /^https?:/.test(target) ? ' rel="noopener"' : ""
        return `<a href="${escapeHtml(target)}"${titleAttr}${external}>${text}</a>`
      },
    },
  })
  // Wide command tables scroll inside the page instead of stretching it.
  return marked
    .parse(markdown)
    .replaceAll("<table>", '<div class="doc-table"><table>')
    .replaceAll("</table>", "</table></div>")
}

const NAV = (current) =>
  PAGES.map(
    (page) =>
      `        <a href="/docs/${page.slug}.html"${page.slug === current ? ' aria-current="page"' : ""}>${page.label}</a>`,
  ).join("\n")

const shell = ({ slug: current, title, description, canonical, body }) => `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>${escapeHtml(title)} · Dalo documentation</title>
  <meta name="description" content="${escapeHtml(description)}" />
  <link rel="canonical" href="${canonical}" />
  <meta name="theme-color" content="#f8f9fa" media="(prefers-color-scheme: light)" />
  <meta name="theme-color" content="#15171c" media="(prefers-color-scheme: dark)" />
  <meta property="og:type" content="article" />
  <meta property="og:site_name" content="Dalo" />
  <meta property="og:url" content="${canonical}" />
  <meta property="og:title" content="${escapeHtml(title)} · Dalo documentation" />
  <meta property="og:description" content="${escapeHtml(description)}" />
  <meta property="og:image" content="${SITE}/assets/img/og.png" />
  <link rel="icon" href="/assets/img/favicon.svg" type="image/svg+xml" />
  <link rel="icon" href="/assets/img/favicon.ico" sizes="any" />
  <link rel="apple-touch-icon" href="/assets/img/apple-touch-icon.png" />
  <link rel="preload" href="/assets/fonts/hanken-400.woff2" as="font" type="font/woff2" crossorigin />
  <link rel="preload" href="/assets/fonts/geistmono-400.woff2" as="font" type="font/woff2" crossorigin />
  <link rel="stylesheet" href="/styles.css" />
  <link rel="stylesheet" href="/docs.css" />
</head>
<body class="doc-page">
<a class="skip-link" href="#main">Skip to content</a>

<header class="site-header" data-scope="dark">
  <div class="wrap header-inner">
    <a class="brand" href="/" aria-label="Dalo home">
      <svg class="brand-mark" viewBox="0 0 32 32" width="30" height="30" aria-hidden="true" focusable="false">
        <rect x="1.25" y="1.25" width="29.5" height="29.5" rx="8" fill="none" stroke="currentColor" stroke-width="1.5" />
        <circle cx="9" cy="9.5" r="2" fill="currentColor" />
        <circle cx="9" cy="16" r="2" fill="currentColor" />
        <circle cx="9" cy="22.5" r="2" fill="currentColor" />
        <path d="M11 9.5 H17 Q22 9.5 22 16 Q22 22.5 17 22.5 H11 M11 16 H22" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" opacity="0.55" />
        <circle cx="22.5" cy="16" r="2.6" fill="var(--accent)" />
      </svg>
      <span class="brand-word">dalo</span>
      <span class="brand-section">docs</span>
    </a>
    <nav class="site-nav" aria-label="Primary">
      <a href="/#how">How it works</a>
      <a href="/#quickstart">Quickstart</a>
      <a href="/docs/">Documentation</a>
    </nav>
    <div class="header-actions">
      <a class="ghost-btn" href="${REPO}" rel="noopener">
        <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"/></svg>
        <span>GitHub</span>
      </a>
    </div>
  </div>
</header>

<main id="main" class="doc-shell wrap">
  <nav class="doc-nav" aria-label="Documentation">
    <p class="footer-h">Documentation</p>
${NAV(current)}
  </nav>
  <article class="doc-body">
${body}
  </article>
</main>

<footer class="site-footer doc-footer" data-scope="dark">
  <div class="wrap footer-base">
    <span>© 2026 <a href="https://sebastian-software.de" rel="noopener">Sebastian Software</a> · Dalo</span>
    <span><a href="/" >dalo.sh</a> · <a href="${REPO}" rel="noopener">GitHub</a> · <a href="${REPO}/blob/main/CHANGELOG.md" rel="noopener">Changelog</a></span>
  </div>
</footer>
</body>
</html>
`

const docPage = async (page) => {
  const markdown = await readFile(path.join(docsSourceDir, `${page.slug}.md`), "utf8")
  const title = markdown.match(/^#\s+(.+)$/m)?.[1]?.trim()
  if (!title) throw new Error(`docs/${page.slug}.md has no level-1 heading`)
  const body = renderMarkdown(markdown).trimEnd()
  const source = `${BLOB}/docs/${page.slug}.md`
  return shell({
    slug: page.slug,
    title,
    description: page.summary,
    canonical: `${SITE}/docs/${page.slug}.html`,
    body: `${body}\n<p class="doc-source">Source: <a href="${source}" rel="noopener">docs/${page.slug}.md</a></p>`,
  })
}

const indexPage = () => {
  const items = PAGES.map(
    (page) => `<li>
  <a class="doc-card" href="/docs/${page.slug}.html">
    <span class="doc-card-title">${page.label}</span>
    <span class="doc-card-summary">${escapeHtml(page.summary)}</span>
  </a>
</li>`,
  ).join("\n")
  return shell({
    slug: "index",
    title: "Documentation",
    description: "Dalo documentation: getting started, command reference, agent integration, CI, troubleshooting, and uninstall.",
    canonical: `${SITE}/docs/`,
    body: `<h1>Dalo documentation</h1>
<p class="doc-lede">Everything the CLI can do today. The installation guide lives at
<a href="/install.md">install.md</a>, the release history in the
<a href="${BLOB}/CHANGELOG.md" rel="noopener">changelog</a>.</p>
<ul class="doc-cards">
${items}
</ul>`,
  })
}

const readVersion = async () => {
  const manifest = await readFile(path.join(rootDir, "Cargo.toml"), "utf8")
  const version = manifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
  if (!version) throw new Error("no version in Cargo.toml")
  return version
}

// Keep the checked-in landing page free of deploy-time placeholders: the
// version lives in real, annotated slots that release-please and this script
// both know how to update. The legacy `__DALO_VERSION__` token is still
// substituted so a page that has not been converted yet keeps working.
const VERSION_TOKEN = "__DALO_VERSION__"
const LASTMOD_TOKEN = "__DALO_LASTMOD__"

const stampVersion = (html, version) => {
  let replacements = 0
  const stamped = html
    .replace(/(<span data-dalo-version>)[^<]*(<\/span>)/g, (_match, open, close) => {
      replacements += 1
      return `${open}${version}${close}`
    })
    .replace(/("softwareVersion":\s*")[^"]*(")/g, (_match, open, close) => {
      replacements += 1
      return `${open}${version}${close}`
    })
    .replaceAll(VERSION_TOKEN, version)
  if (replacements < 3) throw new Error(`site/index.html lost its version slots (found ${replacements})`)
  return stamped
}

const collect = async () => {
  const version = await readVersion()
  const lastmod = new Date().toISOString().slice(0, 10)
  const files = new Map()
  for (const page of PAGES) files.set(`docs/${page.slug}.html`, await docPage(page))
  files.set("docs/index.html", indexPage())
  files.set("index.html", stampVersion(await readFile(path.join(siteDir, "index.html"), "utf8"), version))
  files.set(
    "sitemap.xml",
    (await readFile(path.join(siteDir, "sitemap.xml"), "utf8")).replaceAll(LASTMOD_TOKEN, lastmod),
  )
  return { files, version, lastmod }
}

const main = async () => {
  const check = process.argv.includes("--check")
  const { files, version, lastmod } = await collect()

  // Tracked outputs: the rendered documentation and the stamped landing page.
  const tracked = [...files.keys()].filter((name) => name !== "sitemap.xml")
  const drift = []
  for (const name of tracked) {
    const target = path.join(siteDir, name)
    const current = await readFile(target, "utf8").catch(() => null)
    if (current === files.get(name)) continue
    drift.push(name)
    if (!check) {
      await mkdir(path.dirname(target), { recursive: true })
      await writeFile(target, files.get(name))
    }
  }

  if (check) {
    if (drift.length > 0) {
      console.error(`stale site output: ${drift.join(", ")}`)
      console.error("run `node site/build.mjs` and commit the result")
      process.exitCode = 1
      return
    }
    console.log(`site output is current (dalo ${version})`)
    return
  }

  // site/build/ is the deployable tree: the checked-in site plus deploy-time
  // values that must not live in the repository.
  await rm(buildDir, { recursive: true, force: true })
  await mkdir(buildDir, { recursive: true })
  for (const entry of await readdir(siteDir, { withFileTypes: true })) {
    if (NOT_DEPLOYED.has(entry.name)) continue
    await cp(path.join(siteDir, entry.name), path.join(buildDir, entry.name), { recursive: true })
  }
  await writeFile(path.join(buildDir, "sitemap.xml"), files.get("sitemap.xml"))

  const changed = drift.length > 0 ? `updated ${drift.join(", ")}; ` : ""
  console.log(`${changed}built site/build/ for dalo ${version} (lastmod ${lastmod})`)
}

await main()
