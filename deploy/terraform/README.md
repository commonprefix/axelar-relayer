# Terraform & Overview

## Preparation

Ensure that you have appropriate AWS credentials in profile `[relayer-profile]`.

## Workspaces

Workspaces define the environment for which we are setting up terraform. They include:

- `xrpl-devnet`
- `xrpl-testnet`
- `xrpl-mainnet`
- `ton-devnet`
- `ton-testnet`
- `ton-mainnet`
- `solana-devnet`
- `solana-testnet`
- `solana-mainnet`

To check if your workspace exists, run:

```terraform workspace list```

If it does not, create it with:

```terraform workspace new <WORKSPACE_NAME>```

After this, whenever you want to be in this workspace, run:

```terraform workspace select <WORKSPACE_NAME>```

## Initialization

When you first checkout this repository, or if any new providers were added, you will need to initialize the working
Terraform directory. This can be done with:

```
terraform init -backend-config="profile=relayer-profile"
```

After this, it is smart to run:

```terraform plan```

to make sure there are no pending changes.

## Roles & Policies

Defining roles and policies in Terraform has some gotchas - as you will likely using existing policies, if you connect
them
via aws_iam_policy_attachment - you will detach them from other roles. Perhaps it is best to leave IAM outside of
Terraform unless it is fully managed.

## Region and AZ Affinity

This Terraform project prefers `us-east-1`. This reagion is usually the the cheapest and features come first to it.

We should aim to have region and availability zone (AZ) affinity, unless horizontal scaling requires cross-AZ
deployments.

Please remember that AZs are named differently across accounts. So, us-east-1a in one account, may map to us-east-1c in
another. Instead, please use ZoneId as the identifier of the zone.

To map AZ names to ZoneIDs run:

```
aws ec2 describe-availability-zones --region us-east-1
```

# Setup

## Secrets manager

For secrets, we are inspired by Kubernetes solution for loading secrets: entire secret is dumped into a file which is
made available in the container at runtime.

While it would be more elegant to fetch secrets directly from Rust, this would tie us down to AWS for deployments and we
might have to introduce a logic for different configuration loading for development and deployed services.

Terraform will create empty secrets - you should populate them from AWS Console by saving the entire file contents in a
secret as Plaintext. Key and certificate will be saved in
`certs/client.crt` and `certs/client.key`.

So please modify your configuration to always point to:

```
client_cert_path: "certs/ton.crt"
client_key_path: "certs/ton.key"
```

Once we deploy to ECS, we will also move everything from .env into a structured (key => value) secret in Secrets Manager
and load it as environment variable, if still needed.

## IAM

To fetch secrets, the AWS IAM Role **`relayer_role`** must first be assumed.  
This role has the attached policy **`relayer_secrets_policy`**, which grants the necessary permissions to access the
secrets.

### Who can assume this role?

- When Relayer is deployed to **EC2** or **ECS**, we can configure the corresponding instance or task roles to assume
  `relayer_role`.
- A more secure, long-term solution for workloads outside AWS would be to use [**IAM Roles Anywhere
  **](https://aws.amazon.com/blogs/security/use-iam-roles-anywhere-to-help-you-improve-security-in-on-premises-container-workloads/),
  which provides temporary credentials without requiring an IAM user.

### Current (temporary) solution

Since setting up IAM Roles Anywhere would require signing and maintaining a certificate, a simpler temporary approach is
to create an **IAM user `relayer_user`**.

- This user is allowed to assume `relayer_role`.
- Applications can authenticate as this user and then assume the role to access secrets.  
