const ROBOTS = "User-agent: *\nDisallow: /\n";
const ROBOTS_POLICY = "noindex, nofollow, noarchive";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/robots.txt") {
      return new Response(ROBOTS, {
        headers: {
          "Cache-Control": "no-store",
          "Content-Type": "text/plain; charset=utf-8",
          "X-Robots-Tag": ROBOTS_POLICY,
        },
      });
    }

    const response = await env.ASSETS.fetch(request);
    const headers = new Headers(response.headers);
    if (url.pathname === "/.well-known/security.txt") {
      headers.set("Content-Type", "text/plain; charset=utf-8");
    }
    headers.set("X-Robots-Tag", ROBOTS_POLICY);
    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers,
    });
  },
};
