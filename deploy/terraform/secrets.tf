resource "aws_secretsmanager_secret" "relayer_config" {
  name = "relayer/${terraform.workspace}-config"
}

resource "aws_secretsmanager_secret" "relayer_certificate" {
  name = "relayer/${terraform.workspace}-certificate"
}

resource "aws_secretsmanager_secret" "relayer_key" {
  name = "relayer/${terraform.workspace}-key"
}
