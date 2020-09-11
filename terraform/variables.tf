variable "private_key_path" {
  type        = string
  description = "SSH private key path"
  default     = "/home/tiago/.ssh/id_rsa"
}

variable "ssh_key" {
  type        = string
  description = "SSH pub key to use"
  default     = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDLiP7L7Lm9T01qkqg3NwzYqffzeA5bk67h21DtpK4nbogEm6oTg7mysUoDyoUzDfFwvijlOyPbs48NyIwqGeOu9HDVOG2ZKbcfcxmBr4VLqLDB2LZp6896dcMA2BAHDxsb4PEjNRkjy5+WZdOezALzu3yrfMpoyAgNbe1Ip2CcFrcJjxgsRjF/hhOyWiJONtUlRG6g24eBP13dvetd3DmfAPtnZOIX3MY5XCHQtu7AofR6G0/0qXtBzNAdlqNs+qpljkTT87HISsR094PmwrN9gvHUd+3OUdbzLhkvexPFq3iMJX1BTAa+Irc5nOuENQGnCf7RZGjV62/fQ6pRJtJo0v3KJjvVLtRZqXWaEeK0GA5BXP8F6ZXUgYYk27ZUBk/VJFFNU0e5+Vq4RBWMi4xE7Ht8s/u7k+6Lnqhe84NAlcdV1EptL+ebGGySIvWVKcbJ7TgJt0HHtkNb4xKtIRtYGobA4RCXR158rSOnK5X3GA+e2qewuGu8dgRIlNa5iYc= tiago@tigmo-mj"
}

variable "ssh_user" {
  type        = string
  description = "The user that should be created and who has sudo power"
  default     = "tiago"
}

variable "image_path" {
  type        = string
  description = "Where the images will be stored"
  default     = "/images"
}

variable "disk_size" {
  type        = number
  description = "The size of the root disk in bytes"
  default     = 6442450944
}

variable "hostname_formatter" {
  type    = string
  default = "ksnode-%d"
}

variable "num_nodes" {
  type        = number
  default     = 3
  description = "The number of nodes to create (should be > 1)"
}

variable "qcow2_image" {
  type        = string
  description = "Ubuntu image for VMs - only needed for libvirt provider"
  default     = "/ubuntu-18.04-server-cloudimg-amd64.img"
}

variable "overlay_cidr" {
  type        = string
  description = "CIDR, classless inter-domain routing"
  default     = "10.244.0.0/16"
}

variable "nr_hugepages" {
  type        = string
  description = "Number of Huge pages"
  default     = "512"
}

variable "modprobe_nvme" {
  type        = string
  description = "modprobe nvme tcp selector for node.sh"
  #default     = "ubuntu-20.04-server-cloudimg-amd64.img"
  default     = "none"
}

