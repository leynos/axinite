import { access, cp, mkdir, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const DIST_DIR = path.join(process.cwd(), "dist");
const DEPLOY_BASE_PATH = "/axinite-mockup/";
const DEPLOY_DIR = path.join(
  DIST_DIR,
  DEPLOY_BASE_PATH.replace(/^\/+|\/+$/g, "")
);
const ROUTES = ["chat", "memory", "jobs", "routines", "extensions", "skills"];

export async function postbuildRoutes() {
  const indexPath = path.join(DIST_DIR, "index.html");

  try {
    await access(indexPath);
  } catch {
    console.warn(
      "[postbuild-routes] dist/index.html not found — skipping (expected during watch startup)"
    );
    return;
  }

  try {
    await writeFile(path.join(DIST_DIR, ".nojekyll"), "");

    // GitHub Pages project deploys for this repo serve both `/chat` and
    // `/axinite-mockup/chat` from the published artifact tree. Keep full SPA
    // entry points at the root route folders, then mirror the complete build
    // under the deploy-prefixed directory so deep links and reloads work in
    // both layouts without self-redirect stubs.
    for (const route of ROUTES) {
      const routeDir = path.join(DIST_DIR, route);
      await mkdir(routeDir, { recursive: true });
      await cp(indexPath, path.join(routeDir, "index.html"));
    }

    await mkdir(DEPLOY_DIR, { recursive: true });

    const topLevelEntries = await readdir(DIST_DIR, { withFileTypes: true });

    for (const entry of topLevelEntries) {
      if (entry.name === path.basename(DEPLOY_DIR)) {
        continue;
      }

      const sourcePath = path.join(DIST_DIR, entry.name);
      const targetPath = path.join(DEPLOY_DIR, entry.name);

      await cp(sourcePath, targetPath, { recursive: true });
    }
  } catch (error) {
    // During watch-mode rebuilds the dist tree can be in a transient state
    // (partially cleaned or still being written). Log and continue so the
    // dev server stays alive — the next successful build will run this again.
    console.warn(
      "[postbuild-routes] mirroring failed (transient during watch rebuild):",
      error.message
    );
  }
}

async function main() {
  await postbuildRoutes();
  console.log("postbuild routes completed successfully");
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
