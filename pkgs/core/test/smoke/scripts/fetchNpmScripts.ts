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

// Read annotations before opening the output stream, which truncates the file.
const dataPath = path.resolve(import.meta.dirname, "../data/top-npm-scripts.jsonl");
const rejections = new Map<string, string>();
if (fs.existsSync(dataPath)) {
  for (const line of fs.readFileSync(dataPath, "utf8").split("\n").filter(Boolean)) {
    const record = JSON.parse(line) as { script: string; rejection?: string };
    if (record.rejection) rejections.set(record.script, record.rejection);
  }
}

const packages = [...topDownload].sort();
const processedScripts = new Set<string>();
const answers = new Map<number, string[] | null>();
let notify: (() => void) | undefined;

async function* orderedScripts() {
  for (let index = 0; index < packages.length; index++) {
    while (!answers.has(index)) {
      await new Promise<void>((resolve) => {
        notify = resolve;
      });
      notify = undefined;
    }

    const scripts = answers.get(index);
    answers.delete(index);
    if (!scripts) continue;

    let added = 0;
    for (const script of scripts) {
      if (processedScripts.has(script)) continue;
      processedScripts.add(script);
      added++;
      // Pipeline applies stream backpressure here, independently of downloads.
      yield JSON.stringify({ script, rejection: rejections.get(script) }) + "\n";
    }
    console.log(
      `🟢️ Processed ${index}/${packages.length} (${packages[index]}). Total: ${processedScripts.size} (+${added})`,
    );
  }
}

const dataStream = fs.createWriteStream(dataPath);
const writing = stream.pipeline(orderedScripts(), dataStream);

await Promise.all([
  writing,
  promiseQueue(
    packages.map((pkgName, index) => async () => {
      const no = `${index}/${packages.length}`;
      console.log(`🔷️ Downloading ${no} (${pkgName})`);

      let scripts: string[] | null = null;
      try {
        const response = await fetch(
          `https://registry.npmjs.org/${encodeURIComponent(pkgName)}/latest`,
          {
            headers: {
              Accept: "application/json",
              ...(process.env.NPM_TOKEN
                ? { Authorization: `Bearer ${process.env.NPM_TOKEN}` }
                : {}),
            },
            signal: AbortSignal.timeout(30_000),
            redirect: "error",
          },
        );

        if (!response.ok) {
          await response.body?.cancel();
          throw new Error(`HTTP ${response.status} ${response.statusText}`);
        }

        const result: unknown = await response.json();
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

        const values = parsed.data.scripts ?? {};
        scripts = Object.keys(values)
          .sort()
          .map((name) => values[name]!);
      } catch (error) {
        console.error(`🔴️ Failed to download package.json for ${pkgName}: ${error}`);
      } finally {
        // Failed requests occupy their index too, so they cannot stall the writer.
        // Enqueuing is synchronous: this task releases its download slot immediately.
        answers.set(index, scripts);
        notify?.();
      }
    }),
    3,
  ),
]);
