# infra/aws

AWS resources for `apps/engine`.

## Scope

Following `apps/engine/ARCHITECTURE.md`:
- AWS Organizations accounts (management, staging, production) and IAM Identity Center.
- The VPC: public subnets for the load balancer and NAT gateway, private subnets for tasks and the database.
- ECS on Fargate: the `push-api` and `push-worker` services, their task roles and autoscaling.
- The PostgreSQL database, its backups and cross-region snapshot copies.
- The SQS dispatch queue and its dead-letter queue.
- The two KMS keys and their key policies.
- The Application Load Balancer, WAF and ACM certificate.
- ECR, CloudWatch alarms and dashboards, and the GitHub OIDC deploy roles.

Customer secrets never pass through infrastructure code or its state. Empty until the IaC tool is chosen.
