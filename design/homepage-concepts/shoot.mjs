// Screenshots every concept: node design/homepage-concepts/shoot.mjs [slug...]
// Needs Playwright (NODE_PATH="$(npm root -g)" when installed globally).
import { chromium } from "playwright"
import { readdirSync, mkdirSync } from "node:fs"
import path from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

const dir = path.dirname(fileURLToPath(import.meta.url))
const out = path.join(dir, "screenshots")
mkdirSync(out, { recursive: true })
const only = process.argv.slice(2)
const concepts = readdirSync(dir).filter((d) => /^\d\d-/.test(d) && (!only.length || only.some((o) => d.includes(o))))

const browser = await chromium.launch()
for (const slug of concepts) {
  const url = pathToFileURL(path.join(dir, slug, "index.html")).href
  for (const [name, viewport, scale] of [["desktop", { width: 1440, height: 900 }, 1], ["mobile", { width: 390, height: 844 }, 2]]) {
    const page = await browser.newPage({ viewport, deviceScaleFactor: scale, reducedMotion: "reduce" })
    await page.goto(url, { waitUntil: "networkidle" })
    await page.evaluate(() => document.fonts.ready)
    await page.screenshot({ path: path.join(out, `${slug}-${name}-hero.png`), fullPage: false })
    if (name === "desktop") await page.screenshot({ path: path.join(out, `${slug}-${name}-full.jpg`), fullPage: true, type: "jpeg", quality: 82 })
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
    if (overflow > 0) console.warn(`${slug} ${name}: horizontal overflow ${overflow}px`)
    await page.close()
  }
  console.log("shot", slug)
}
await browser.close()
