resource "aws_iam_role" "relayer_role" {
  name = "relayer-role-${terraform.workspace}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Principal = {
          Service = [
            "ec2.amazonaws.com",
            "ecs-tasks.amazonaws.com"
          ]
        }
        Action = "sts:AssumeRole"
      },
      {
        Effect = "Allow"
        Principal = {
          AWS = aws_iam_user.relayer_user.arn
        }
        Action = "sts:AssumeRole"
      }
    ]
  })
}

resource "aws_iam_policy" "relayer_secrets_policy" {
  name        = "relayer-secrets-policy-${terraform.workspace}"
  description = "Allow access to relayer/${terraform.workspace}-* secrets"

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "secretsmanager:GetSecretValue",
          "secretsmanager:DescribeSecret"
        ]
        Resource = "arn:aws:secretsmanager:*:*:secret:relayer/${terraform.workspace}-*"
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "relayer_secrets_attach" {
  role       = aws_iam_role.relayer_role.name
  policy_arn = aws_iam_policy.relayer_secrets_policy.arn
}

resource "aws_iam_user" "relayer_user" {
  name = "relayer_${terraform.workspace}"
}

resource "aws_iam_user_policy" "relayer_user_assume_role" {
  name = "relayer-user-assume-role-${terraform.workspace}"
  user = aws_iam_user.relayer_user.name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = "sts:AssumeRole"
        Resource = aws_iam_role.relayer_role.arn
      }
    ]
  })
}

