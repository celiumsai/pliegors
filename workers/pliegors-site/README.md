# PliegoRS site deployment

This package deploys the verified static output from
`examples/pliegors-site/target/site` to the `pliegors.dev` custom domain. It has
two explicit delivery profiles:

- `wrangler.jsonc` is the public profile. Pages remain indexable and the
  generated public `robots.txt` advertises the sitemap.
- `wrangler.preview.jsonc` is the protected-preview profile. Its Worker adds
  `X-Robots-Tag: noindex, nofollow, noarchive` to every response and replaces
  `/robots.txt` with `Disallow: /` without mutating the verified site artifact.

The public `workers.dev` route and version preview URLs are disabled. Cloudflare
Access is configured at the account boundary and must remain deny-by-default;
the deployment package contains no identity policy or credentials.

```sh
npm ci
npm run check
npm run check:preview
npm run deploy:preview
```

Build and inspect the official site before deployment. Do not edit the generated
output directory or deploy an artifact whose `pliego.build.json` ledger fails
verification. Cloudflare Access must remain deny-by-default while the preview
profile is active. Opening the project publicly is a separate, deliberate
`npm run deploy:public` operation after Access and release gates are reviewed.
