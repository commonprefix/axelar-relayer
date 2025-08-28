terraform {
  required_version = ">= 1.11.0"

  backend "s3" {
    bucket       = "relayer-terraform-state"
    use_lockfile = true
    encrypt      = true
    region       = "us-east-1"
    key          = "state"
    profile      = "relayer-terraform-profile"
  }
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "6.10.0"
    }
  }
}

provider "aws" {
  profile = "relayer-terraform-profile"
  region  = "us-east-1"
}
