import { readFile, stat } from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const DEPLOY_BASE = "/axinite-mockup/";
const REQUIRED_OUTPUTS = [
  "dist/index.html",
  "dist/.nojekyll",
  "dist/chat/index.html",
  "dist/memory/index.html",
  "dist/jobs/index.html",
  "dist/routines/index.html",
  "dist/extensions/index.html",
  "dist/skills/index.html",
  "dist/axinite-mockup/index.html",
  "dist/axinite-mockup/chat/index.html",
  "dist/axinite-mockup/memory/index.html",
  "dist/axinite-mockup/jobs/index.html",
  "dist/axinite-mockup/routines/index.html",
  "dist/axinite-mockup/extensions/index.html",
  "dist/axinite-mockup/skills/index.html",
  "dist/axinite-mockup/manifest.webmanifest",
  "dist/manifest.webmanifest",
];

async function main() {
  for (const relativePath of REQUIRED_OUTPUTS) {
    const absolutePath = path.join(ROOT, relativePath);

    try {
      const fileStat = await stat(absolutePath);

      if (!fileStat.isFile()) {
        throw new Error("not a file");
      }
    } catch {
      console.error(`Missing required build output: ${relativePath}`);
      process.exitCode = 1;
      return;
    }
  }

  const manifestPath = path.join(ROOT, "dist", "manifest.webmanifest");
  let manifest;

  try {
    manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  } catch (error) {
    console.error(
      `Unable to read or parse manifest at ${manifestPath}: ${error.message}`
    );
    process.exitCode = 1;
    return;
  }

  if (manifest.scope !== DEPLOY_BASE) {
    console.error(`Manifest scope must be ${DEPLOY_BASE}`);
    process.exitCode = 1;
    return;
  }

  if (manifest.start_url !== `${DEPLOY_BASE}chat`) {
    console.error(`Manifest start_url must be ${DEPLOY_BASE}chat`);
    process.exitCode = 1;
    return;
  }

  if (!Array.isArray(manifest.icons) || manifest.icons.length === 0) {
    console.error("Manifest must contain a non-empty icons array");
    process.exitCode = 1;
    return;
  }

  const iconSrc = manifest.icons[0]?.src;
  if (iconSrc !== `${DEPLOY_BASE}assets/icons/axinite32.ico`) {
    console.error(
      `Manifest icon path must be ${DEPLOY_BASE}assets/icons/axinite32.ico`
    );
    process.exitCode = 1;
    return;
  }

  const indexHtml = await readFile(
    path.join(ROOT, "dist", "index.html"),
    "utf8"
  );
  if (
    !indexHtml.includes('src="/axinite-mockup/assets/') ||
    !indexHtml.includes('href="/axinite-mockup/assets/')
  ) {
    console.error(
      "Built index.html must reference assets under /axinite-mockup/"
    );
    process.exitCode = 1;
    return;
  }

  const rootRouteCompatibilityHtml = await readFile(
    path.join(ROOT, "dist", "chat", "index.html"),
    "utf8"
  );
  if (!rootRouteCompatibilityHtml.includes('src="/axinite-mockup/assets/')) {
    console.error(
      "Root route compatibility pages must preserve /axinite-mockup/ asset URLs."
    );
    process.exitCode = 1;
    return;
  }

  const deployRouteHtml = await readFile(
    path.join(ROOT, "dist", "axinite-mockup", "chat", "index.html"),
    "utf8"
  );
  if (!deployRouteHtml.includes('src="/axinite-mockup/assets/')) {
    console.error(
      "Deploy-prefixed route pages must preserve /axinite-mockup/ asset URLs."
    );
    process.exitCode = 1;
    return;
  }

  console.log("Build smoke test passed.");
}

await main();
