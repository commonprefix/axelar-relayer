resource "aws_secretsmanager_secret" "relayer_config" {
  name = "relayer/${terraform.workspace}-config"
  tags = {
    Environment = terraform.workspace
  }
}

resource "aws_secretsmanager_secret" "relayer_certificate" {
  name = "relayer/${terraform.workspace}-certificate"
  tags = {
    Environment = terraform.workspace
  }
}

resource "aws_secretsmanager_secret" "relayer_key" {
  name = "relayer/${terraform.workspace}-key"
  tags = {
    Environment = terraform.workspace
  }
}
