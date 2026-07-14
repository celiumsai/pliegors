# PliegoRS site deployment

This package deploys the verified static output from
`examples/pliegors-site/target/site` to the `pliegors.dev` custom domain.

The public `workers.dev` route and version preview URLs are disabled. Cloudflare
Access is configured at the account boundary and must remain deny-by-default;
the deployment package contains no identity policy or credentials.

```sh
npm ci
npm run check
npm run deploy
```

Build and inspect the official site before deployment. Do not edit the generated
output directory or deploy an artifact whose `pliego.build.json` ledger fails
verification.
