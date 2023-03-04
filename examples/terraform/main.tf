
variable "current_dir" {
  type = string
}

variable "parent_dir" {
  type = string
}

output "example" {
  value = "${var.parent_dir} ${var.current_dir}"
}
