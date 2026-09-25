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
const buildDir = path.join(siteDir, "build")

const REPO = "https://github.com/sebastian-software/dalo"
const BLOB = `${REPO}/blob/main`
const SITE = "https://dalo.sh"
const SPEC_VERSION = "0.1"

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
    slug: "team",
    label: "Team repository",
    summary: "Publish a team source: pin external catalogs, advance a pin, and onboard a teammate.",
  },
  {
    slug: "reference",
    label: "Command reference",
    summary: "Every command, flag, config file, JSON report, and diagnostic code.",
  },
  {
    slug: "compatibility",
    label: "Compatibility",
    summary: "What is stable in 1.x, what is experimental, and how breaking changes are announced.",
  },
  {
    slug: "upgrading",
    label: "Upgrading to 1.0",
    summary: "Move a 0.x store to 1.0: what the first sync migrates, which spellings were removed, and how to recover.",
  },
  {
    slug: "plugins",
    label: "Plugins",
    summary:
      "Portable plugin packages, their typed tools and hooks, and the separate approvals each one needs.",
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
  {
    slug: "security",
    label: "Security overview",
    summary: "Trust boundaries, what the preflight blocks, the approval model, and the stated limits.",
  },
  { slug: "uninstall", label: "Uninstall", summary: "Remove targets, autosync, the store, and the binary." },
  {
    slug: "comparison",
    label: "Comparison",
    summary: "How Dalo compares with agentfiles and Vercel's skills CLI, and when each one is the better fit.",
  },
]

const SPEC_PAGES = [
  {
    slug: "index",
    source: "README.md",
    label: "Specification",
    summary: "Portable Agent Packages draft 0.1: the passive package, tool, and hook contract.",
  },
  {
    slug: "compatibility",
    source: "compatibility.md",
    label: "Compatibility",
    summary: "Evidence-backed provider coverage and the limits of the draft 0.1 reference implementation.",
  },
]

const PUBLIC_SOURCE_ROUTES = new Map([
  ["docs/spec/README.md", `/spec/${SPEC_VERSION}/`],
  ["docs/spec/compatibility.md", `/spec/${SPEC_VERSION}/compatibility.html`],
  ["docs/spec/plugin-v1.schema.json", `/spec/${SPEC_VERSION}/plugin-v1.schema.json`],
])

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

// Links are written for the repository; rewrite them for the site. Resolve
// relative paths from the Markdown source first so nested docs retain their
// repository meaning (for example docs/spec/README.md -> docs/adr/...).
const rewriteLink = (href, sourcePath) => {
  if (/^(https?:|mailto:|#)/.test(href)) return href
  const [target, anchor = ""] = href.split("#")
  const suffix = anchor ? `#${anchor}` : ""
  if (target === "") return href

  const sourceDir = path.posix.dirname(sourcePath)
  const resolvedPath = path.posix.normalize(path.posix.join(sourceDir, target))
  const publishedRoute = PUBLIC_SOURCE_ROUTES.get(resolvedPath)
  if (publishedRoute) return `${publishedRoute}${suffix}`
  if (resolvedPath === "site/install.md") return `/install.md${suffix}`

  const docsMatch = resolvedPath.match(/^docs\/([^/]+)\.md$/)
  if (docsMatch && PAGES.some((page) => page.slug === docsMatch[1])) {
    return `/docs/${docsMatch[1]}.html${suffix}`
  }
  return `${BLOB}/${resolvedPath}${suffix}`
}

const renderMarkdown = (markdown, sourcePath) => {
  const seen = new Map()
  const toc = []
  const marked = new Marked({ gfm: true })
  marked.use({
    renderer: {
      heading(token) {
        const content = this.parser.parseInline(token.tokens)
        // The slug comes from the raw heading text, so anchors written against
        // the Markdown sources resolve on the rendered page too.
        const id = uniqueSlug(token.text, seen)
        // Links inside a heading would nest in the table of contents.
        if (token.depth === 2) toc.push({ id, html: content.replace(/<\/?a\b[^>]*>/g, "") })
        const inner =
          token.depth === 1 ? content : `<a class="doc-anchor" href="#${id}">${content}</a>`
        return `<h${token.depth} id="${id}">${inner}</h${token.depth}>\n`
      },
      link(token) {
        const text = this.parser.parseInline(token.tokens)
        const target = rewriteLink(token.href, sourcePath)
        const titleAttr = token.title ? ` title="${escapeHtml(token.title)}"` : ""
        const external = /^https?:/.test(target) ? ' rel="noopener"' : ""
        return `<a href="${escapeHtml(target)}"${titleAttr}${external}>${text}</a>`
      },
    },
  })
  // Wide command tables scroll inside the page instead of stretching it.
  const html = marked
    .parse(markdown)
    .replaceAll("<table>", '<div class="doc-table"><table>')
    .replaceAll("</table>", "</table></div>")
  return { html, toc }
}

// The brand is the homepage's logo and wordmark files, so the header reads the
// same on every page of the site.
const BRAND_MARK = (height) =>
  `<img class="brand-mark" src="/assets/img/logo.svg" width="${Math.round((height * 274) / 240)}" height="${height}" alt="" />`
const BRAND_WORD = (height) =>
  `<img class="brand-word" src="/assets/img/wordmark.svg" width="${Math.round((height * 2873) / 735)}" height="${height}" alt="" />`

const CHEVRON =
  '<svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true" focusable="false"><path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>'

// The overview entry heads each side navigation: the docs index for the
// documentation, the static landing page for the specification.
const OVERVIEW = {
  docs: { slug: "index", label: "Overview", href: "/docs/" },
  spec: { slug: "landing", label: "Overview", href: "/spec/" },
}

const navEntries = (section, pages, prefix) => [
  OVERVIEW[section],
  ...pages.map((page) => ({
    slug: page.slug,
    label: page.label,
    href: `${prefix}${page.slug === "index" ? "" : `${page.slug}.html`}`,
  })),
]

const NAV = (current, entries, indent) =>
  entries
    .map(
      (entry) =>
        `${indent}<a href="${entry.href}"${entry.slug === current ? ' aria-current="page"' : ""}>${entry.label}</a>`,
    )
    .join("\n")

const TOP_NAV = (section, indent) =>
  [
    ["/", "Product"],
    ["/docs/", "Docs"],
    ["/spec/", "Spec"],
    [REPO, "GitHub"],
  ]
    .map(([href, label]) => {
      const external = href.startsWith("http") ? ' rel="noopener"' : ""
      const current = href === `/${section}/` ? ' aria-current="true"' : ""
      return `${indent}<a href="${href}"${external}${current}>${label}</a>`
    })
    .join("\n")

// "On this page": the level-2 headings of a long document, shown beside it on
// wide screens.
const TOC = (toc) =>
  toc.length < 2
    ? ""
    : `
  <nav class="doc-toc" aria-label="On this page">
    <p class="doc-toc-h">On this page</p>
    <ul>
${toc.map((entry) => `      <li><a href="#${entry.id}">${entry.html}</a></li>`).join("\n")}
    </ul>
  </nav>`

const shell = ({
  slug: current,
  title,
  description,
  canonical,
  body,
  toc = [],
  section = "docs",
  navPages = PAGES,
  navPrefix = "/docs/",
}) => {
  const sectionName = section === "spec" ? "Specification" : "Documentation"
  const entries = navEntries(section, navPages, navPrefix)
  const currentLabel = entries.find((entry) => entry.slug === current)?.label ?? escapeHtml(title)
  const tocHtml = TOC(toc)
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>${escapeHtml(title)} · Dalo ${sectionName.toLowerCase()}</title>
  <meta name="description" content="${escapeHtml(description)}" />
  <link rel="canonical" href="${canonical}" />
  <meta name="theme-color" content="#ffffff" />
  <meta property="og:type" content="article" />
  <meta property="og:site_name" content="Dalo" />
  <meta property="og:url" content="${canonical}" />
  <meta property="og:title" content="${escapeHtml(title)} · Dalo ${sectionName.toLowerCase()}" />
  <meta property="og:description" content="${escapeHtml(description)}" />
  <meta property="og:image" content="${SITE}/assets/img/og.png" />
  <link rel="icon" href="/assets/img/favicon.svg" type="image/svg+xml" />
  <link rel="icon" href="/assets/img/favicon.ico" sizes="any" />
  <link rel="apple-touch-icon" href="/assets/img/apple-touch-icon.png" />
  <link rel="stylesheet" href="/styles.css" />
  <link rel="stylesheet" href="/docs.css" />
</head>
<body class="doc-page">
<a class="skip-link" href="#main">Skip to content</a>

<header class="site-header">
  <div class="wrap">
    <a class="brand" href="/" aria-label="Dalo home">
      ${BRAND_MARK(34)}
      ${BRAND_WORD(16)}
      <span class="brand-section">${section}</span>
    </a>
    <nav class="nav site-nav" aria-label="Primary">
${TOP_NAV(section, "      ")}
    </nav>
    <div class="header-cta">
      <a class="pill-btn" href="/docs/getting-started.html">Get started
        ${CHEVRON}
      </a>
      <details class="mobile-menu">
        <summary aria-label="Open navigation">
          <svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true" focusable="false"><path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" d="M2.5 4h11M2.5 8h11M2.5 12h11"/></svg>
          <span class="sr-only">Menu</span>
        </summary>
        <nav class="mobile-nav" aria-label="Mobile">
${TOP_NAV(section, "          ")}
        </nav>
      </details>
    </div>
  </div>
</header>

<main id="main" class="doc-shell wrap${tocHtml ? " has-toc" : ""}">
  <nav class="doc-nav" aria-label="${sectionName}">
    <p class="doc-nav-h">${sectionName}</p>
${NAV(current, entries, "    ")}
  </nav>
  <details class="doc-menu">
    <summary><span class="doc-menu-label">${sectionName}</span> <span class="doc-menu-current">${currentLabel}</span></summary>
    <nav class="doc-menu-nav" aria-label="${sectionName}">
${NAV(current, entries, "      ")}
    </nav>
  </details>
  <article class="doc-body">
${body}
  </article>${tocHtml}
</main>

<footer class="site-footer">
  <div class="wrap">
    <div class="footer-top">
      <div class="footer-brand">
        <a class="brand" href="/" aria-label="Dalo home">
          ${BRAND_MARK(30)}
          ${BRAND_WORD(14)}
        </a>
        <p>Git-backed skill management for AI agents. Built in Rust.</p>
      </div>
      <nav class="footer-cols" aria-label="Footer">
        <div class="footer-col">
          <p class="footer-h">Product</p>
          <a href="/#how">How it works</a>
          <a href="/#quickstart">Quickstart</a>
          <a href="/#compare">Compare</a>
          <a href="/#stability">Stability</a>
        </div>
        <div class="footer-col">
          <p class="footer-h">Docs</p>
          <a href="/docs/">All documentation</a>
          <a href="/docs/getting-started.html">Getting started</a>
          <a href="/docs/reference.html">Reference</a>
          <a href="/docs/troubleshooting.html">Troubleshooting</a>
          <a href="/spec/">Spec</a>
          <a href="/docs/security.html">Security</a>
          <a href="/install.md">Install guide</a>
          <a href="${REPO}/releases" rel="noopener">Releases</a>
        </div>
        <div class="footer-col">
          <p class="footer-h">Project</p>
          <a href="${REPO}" rel="noopener">GitHub</a>
          <a href="${BLOB}/CHANGELOG.md" rel="noopener">Changelog</a>
          <a href="${REPO}/security/policy" rel="noopener">Security policy</a>
          <a href="${BLOB}/LICENSE-MIT" rel="noopener">MIT license</a>
          <a href="${BLOB}/LICENSE-APACHE" rel="noopener">Apache-2.0 license</a>
          <a href="${REPO}/issues" rel="noopener">Issues</a>
        </div>
      </nav>
    </div>
    <div class="footer-base">
      <span>© 2026 <a href="https://sebastian-software.de" rel="noopener">Sebastian Software</a> · No tracking, no cookies</span>
      <span>From the same workshop: <a href="https://github.com/sebastian-software/harness-relay" rel="noopener">harness-relay</a> · <a href="https://oss.sebastian-software.com" rel="noopener">more open source</a></span>
    </div>
  </div>
</footer>
</body>
</html>
`
}

const docPage = async (page) => {
  const sourcePath = `docs/${page.slug}.md`
  const markdown = await readFile(path.join(rootDir, sourcePath), "utf8")
  const title = markdown.match(/^#\s+(.+)$/m)?.[1]?.trim()
  if (!title) throw new Error(`docs/${page.slug}.md has no level-1 heading`)
  const { html, toc } = renderMarkdown(markdown, sourcePath)
  const body = html.trimEnd()
  const source = `${BLOB}/${sourcePath}`
  return shell({
    slug: page.slug,
    title,
    description: page.summary,
    canonical: `${SITE}/docs/${page.slug}.html`,
    toc,
    body: `${body}\n<p class="doc-source">Source: <a href="${source}" rel="noopener">${sourcePath}</a></p>`,
  })
}

const specPage = async (page) => {
  const sourcePath = `docs/spec/${page.source}`
  const markdown = await readFile(path.join(rootDir, sourcePath), "utf8")
  const title = markdown.match(/^#\s+(.+)$/m)?.[1]?.trim()
  if (!title) throw new Error(`${sourcePath} has no level-1 heading`)
  const { html, toc } = renderMarkdown(markdown, sourcePath)
  const body = html.trimEnd()
  const outputPath = page.slug === "index" ? `spec/${SPEC_VERSION}/` : `spec/${SPEC_VERSION}/${page.slug}.html`
  const source = `${BLOB}/${sourcePath}`
  return shell({
    slug: page.slug,
    title,
    description: page.summary,
    canonical: `${SITE}/${outputPath}`,
    toc,
    section: "spec",
    navPages: SPEC_PAGES,
    navPrefix: `/spec/${SPEC_VERSION}/`,
    body: `${body}\n<p class="doc-source">Source: <a href="${source}" rel="noopener">${sourcePath}</a> · <a href="/spec/${SPEC_VERSION}/plugin-v1.schema.json">Download JSON Schema</a></p>`,
  })
}

const specIndexPage = () =>
  shell({
    slug: "landing",
    title: "Portable Agent Packages",
    description: "The experimental Portable Agent Packages draft 0.1 for passive skills, tools, instructions, and hooks.",
    canonical: `${SITE}/spec/`,
    section: "spec",
    navPages: SPEC_PAGES,
    navPrefix: `/spec/${SPEC_VERSION}/`,
    body: `<p class="doc-kicker">Experimental · draft ${SPEC_VERSION}</p>
<h1>Portable Agent Packages</h1>
<p class="doc-lede">An experimental author-facing profile for grouping skills, standing instructions, local tools, and event handlers into a package that a consumer can validate and review.</p>
<ul class="doc-cards">
<li>
  <a class="doc-card" href="/spec/${SPEC_VERSION}/">
    <span class="doc-card-title">Draft ${SPEC_VERSION}</span>
    <span class="doc-card-summary">Read the versioned specification, contract fields, validation stages, and bounded hook mapping.</span>
  </a>
</li>
<li>
  <a class="doc-card" href="/spec/${SPEC_VERSION}/compatibility.html">
    <span class="doc-card-title">Compatibility</span>
    <span class="doc-card-summary">See provider evidence and the limits of the current reference fixtures.</span>
  </a>
</li>
<li>
  <a class="doc-card" href="/spec/${SPEC_VERSION}/plugin-v1.schema.json">
    <span class="doc-card-title">JSON Schema</span>
    <span class="doc-card-summary">Download <code>plugin-v1.schema.json</code> for editor assistance and structural validation.</span>
  </a>
</li>
</ul>`,
  })

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
    body: `<p class="doc-kicker">Documentation</p>
<h1>Dalo documentation</h1>
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
  for (const page of SPEC_PAGES) {
    const output = page.slug === "index" ? `spec/${SPEC_VERSION}/index.html` : `spec/${SPEC_VERSION}/${page.slug}.html`
    files.set(output, await specPage(page))
  }
  files.set("spec/index.html", specIndexPage())
  files.set(
    `spec/${SPEC_VERSION}/plugin-v1.schema.json`,
    await readFile(path.join(docsSourceDir, "spec/plugin-v1.schema.json"), "utf8"),
  )
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
  // The release count on the landing page follows CHANGELOG.md at deploy time,
  // so the published number never lags behind a release-please release.
  const releases = (await readFile(path.join(rootDir, "CHANGELOG.md"), "utf8")).match(/^## \[/gm)?.length ?? 0
  const landing = path.join(buildDir, "index.html")
  const stampedReleases = (await readFile(landing, "utf8")).replace(
    /(<strong data-dalo-releases>)[^<]*(<\/strong>)/,
    (_match, open, close) => `${open}${releases}${close}`,
  )
  await writeFile(landing, stampedReleases)

  const changed = drift.length > 0 ? `updated ${drift.join(", ")}; ` : ""
  console.log(`${changed}built site/build/ for dalo ${version} (lastmod ${lastmod}, ${releases} releases)`)
}

await main()
