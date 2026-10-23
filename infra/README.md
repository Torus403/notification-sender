# infra

Infrastructure as code.
- `aws/`: the engine's AWS resources (compute, Postgres, queues, IAM role for GitHub OIDC deploys).
- `cloudflare/`: account-level Cloudflare setup (DNS, zones, shared bindings). Each app keeps its own `wrangler.jsonc`.

Empty for now. The IaC tool (CDK, Terraform/OpenTofu or SST) and the engine's AWS compute are still open decisions.
