import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { join, extname } from "node:path";

const PORT = parseInt(process.env.PORT || "8080", 10);
const HERALD_URL = (process.env.HERALD_URL || "http://localhost:6201").replace(
  /\/$/,
  "",
);
const DIST = new URL("./dist", import.meta.url).pathname;

const MIME = {
  ".html": "text/html",
  ".js": "application/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
};

async function serveStatic(res, urlPath) {
  const filePath = join(DIST, urlPath === "/" ? "index.html" : urlPath);
  try {
    const s = await stat(filePath);
    if (!s.isFile()) throw new Error();
    const ext = extname(filePath);
    const content = await readFile(filePath);
    res.writeHead(200, { "Content-Type": MIME[ext] || "application/octet-stream" });
    res.end(content);
  } catch {
    // SPA fallback
    const index = await readFile(join(DIST, "index.html"));
    res.writeHead(200, { "Content-Type": "text/html" });
    res.end(index);
  }
}

function proxyToHerald(req, res) {
  const targetPath = req.url.replace(/^\/api/, "");
  const targetUrl = `${HERALD_URL}${targetPath}`;

  const headers = { ...req.headers };
  delete headers.host;

  // Support token as query param (for EventSource which can't set headers)
  const parsed = new URL(targetUrl);
  const qpToken = parsed.searchParams.get("token");
  if (qpToken && !headers.authorization) {
    headers.authorization = `Bearer ${qpToken}`;
    parsed.searchParams.delete("token");
  }

  const cleanUrl = parsed.toString();
  const mod = parsed.protocol === "https:" ? "node:https" : "node:http";

  import(mod).then(({ request }) => {
    const proxyReq = request(
      cleanUrl,
      { method: req.method, headers },
      (proxyRes) => {
        // SSE: stream through
        if (
          proxyRes.headers["content-type"]?.includes("text/event-stream")
        ) {
          res.writeHead(proxyRes.statusCode, {
            "Content-Type": "text/event-stream",
            "Cache-Control": "no-cache",
            Connection: "keep-alive",
          });
          proxyRes.pipe(res);
          return;
        }

        res.writeHead(proxyRes.statusCode, proxyRes.headers);
        proxyRes.pipe(res);
      },
    );

    proxyReq.on("error", (err) => {
      console.error("proxy error:", err.message);
      if (!res.headersSent) {
        res.writeHead(502, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: "upstream unreachable" }));
      }
    });

    req.pipe(proxyReq);
  });
}

const server = createServer((req, res) => {
  if (req.url.startsWith("/api/")) {
    proxyToHerald(req, res);
  } else {
    serveStatic(res, req.url);
  }
});

server.listen(PORT, () => {
  console.log(`Herald Admin UI listening on :${PORT}`);
  console.log(`Proxying /api/* → ${HERALD_URL}`);
});
