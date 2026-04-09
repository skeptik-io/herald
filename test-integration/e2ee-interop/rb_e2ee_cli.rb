#!/usr/bin/env ruby
# frozen_string_literal: true

# Small CLI for E2EE cross-SDK interop testing.
#
# Usage:
#   ruby rb_e2ee_cli.rb encrypt <hex_shared_secret> <version> <plaintext> <stream_id>
#   ruby rb_e2ee_cli.rb decrypt <hex_shared_secret> <version> <ciphertext> <stream_id>
#   ruby rb_e2ee_cli.rb blind   <hex_shared_secret> <plaintext>

require "json"
require_relative "../../herald-admin-ruby/lib/herald_admin/e2ee"

cmd = ARGV[0]

case cmd
when "encrypt"
  secret = [ARGV[1]].pack("H*")
  version = ARGV[2].to_i
  plaintext = ARGV[3]
  stream_id = ARGV[4]
  session = HeraldAdmin::E2EE.create_session(secret, version)
  ct = session.encrypt_convergent(plaintext, stream_id)
  puts JSON.generate(ciphertext: ct)

when "decrypt"
  secret = [ARGV[1]].pack("H*")
  version = ARGV[2].to_i
  ciphertext = ARGV[3]
  stream_id = ARGV[4]
  session = HeraldAdmin::E2EE.create_session(secret, version)
  pt = session.decrypt(ciphertext, stream_id)
  puts JSON.generate(plaintext: pt)

when "blind"
  secret = [ARGV[1]].pack("H*")
  plaintext = ARGV[2]
  session = HeraldAdmin::E2EE.create_session(secret, 1)
  wire = session.blind(plaintext)
  puts JSON.generate(wire: wire)

else
  warn "unknown command: #{cmd}"
  exit 1
end
