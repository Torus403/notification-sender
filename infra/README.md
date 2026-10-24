# infra

Infrastructure as code.

| Path          | Purpose                                                                                                                         |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `aws/`        | The engine's AWS resources: network, ECS services, database, queue, KMS keys, IAM, alarms.                                      |
| `cloudflare/` | Account-level Cloudflare setup shared by the websites: DNS, zones and shared bindings. Each app keeps its own `wrangler.jsonc`. |

Empty for now. The AWS design is in `apps/engine/ARCHITECTURE.md`; the IaC tool (OpenTofu or Terraform) is open decision A4 there.
