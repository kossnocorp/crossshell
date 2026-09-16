#!/usr/bin/env node

import { topDownload } from "../../../vendor/@wooorm/npm-high-impact/lib/top-download.js";
import { promiseQueue } from "@js-fns/promise";
import z from "zod";
import fs from "node:fs";
import path from "node:path";
import stream from "node:stream/promises";

const npmToken = process.env.NPM_TOKEN;
if (!npmToken) {
  console.error("🔴️ NPM_TOKEN is not set");
  process.exit(1);
}

const PackageJson = z.object({
  scripts: z.record(z.string(), z.string()).optional(),
});

const allScripts = new Set<string>();
let processing = 0;

await promiseQueue(
  topDownload.map((pkgName) => async () => {
    const no = `${processing++}/${topDownload.length}`;
    console.log(`🔷️ Downloading ${no} (${pkgName})`);

    let result: unknown;
    try {
      const response = await fetch(
        `https://registry.npmjs.org/${encodeURIComponent(pkgName)}/latest`,
        {
          headers: {
            Accept: "application/json",
            ...(process.env.NPM_TOKEN ? { Authorization: `Bearer ${process.env.NPM_TOKEN}` } : {}),
          },
          signal: AbortSignal.timeout(30_000),
          redirect: "error",
        },
      );

      if (!response.ok) {
        await response.body?.cancel();
        throw new Error(`HTTP ${response.status} ${response.statusText}`);
      }

      result = await response.json();
    } catch (error) {
      console.error(`🔴️ Failed to download package.json for ${pkgName}: ${error}`);
      return;
    }

    const parsed = PackageJson.safeParse(result);
    if (!parsed.success) {
      console.error(`🔴️ Failed to parse package.json for ${pkgName}: ${parsed.error.message}`);
      if (process.env.DEBUG) {
        console.log("╭ package.json ------------------------╮");
        console.log(JSON.stringify(result, null, 2));
        console.log("╰--------------------------------------╯");
      }
      return;
    }

    const values = parsed.data.scripts ? Object.values(parsed.data.scripts) : [];
    values.forEach((script) => allScripts.add(script));

    console.log(`🟢️ Processed ${no} (${pkgName}). Total: ${allScripts.size} (+${values.length})`);
  }),
  3,
);

const dataPath = path.resolve(import.meta.dirname, "../data/top-npm-scripts.sh");
const dataStream = fs.createWriteStream(dataPath);

allScripts.forEach((script) => {
  try {
    dataStream.write(script + "\n");
  } catch (error) {
    console.error(`🔴️ Failed to write data for script ${script}: ${error}`);
    return;
  }
});
dataStream.end();
await stream.finished(dataStream);
