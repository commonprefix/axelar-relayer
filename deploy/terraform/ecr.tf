resource "aws_ecr_repository" "relayer" {
  name = "relayer/${terraform.workspace}"

  image_scanning_configuration {
    scan_on_push = true
  }

  tags = {
    Environment = terraform.workspace
  }
}

resource "aws_ecr_lifecycle_policy" "relayer_retention_90d" {
  repository = aws_ecr_repository.relayer.name

  policy = jsonencode({
    rules = [
      {
        rulePriority = 1
        description  = "Expire untagged images older than 30 days"
        selection = {
          tagStatus   = "untagged"
          countType   = "sinceImagePushed"
          countUnit   = "days"
          countNumber = 30
        }
        action = { type = "expire" }
      },
      {
        rulePriority = 2
        description  = "Expire any images older than 90 days"
        selection = {
          tagStatus = "any"              # covers tagged & untagged
          countType   = "sinceImagePushed"
          countUnit   = "days"
          countNumber = 90
        }
        action = { type = "expire" }
      }
    ]
  })
}
