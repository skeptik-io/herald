# frozen_string_literal: true

require "net/http"
require "json"
require "uri"

module HeraldAdmin
  class HttpTransport
    def initialize(base_url, token)
      @base_url = base_url.chomp("/")
      @token = token
    end

    def request(method, path, body = nil)
      uri = URI("#{@base_url}#{path}")
      req = case method.upcase
            when "GET"    then Net::HTTP::Get.new(uri)
            when "POST"   then Net::HTTP::Post.new(uri)
            when "PATCH"  then Net::HTTP::Patch.new(uri)
            when "DELETE" then Net::HTTP::Delete.new(uri)
            else raise ArgumentError, "unknown method: #{method}"
            end

      req["Authorization"] = "Bearer #{@token}"
      if body
        req["Content-Type"] = "application/json"
        req.body = JSON.generate(body)
      end

      http = Net::HTTP.new(uri.host, uri.port)
      http.use_ssl = uri.scheme == "https"
      http.open_timeout = 10
      http.read_timeout = 30
      resp = http.request(req)

      status = resp.code.to_i
      if status >= 400
        code = "INTERNAL"
        message = resp.body
        begin
          data = JSON.parse(resp.body)
          message = data["error"] if data["error"]
        rescue JSON::ParserError
          # raw text
        end
        code = "UNAUTHORIZED" if status == 401
        code = "NOT_FOUND" if status == 404
        raise HeraldError.new(code, message, status: status)
      end

      return nil if status == 204
      return nil if resp.body.nil? || resp.body.empty?
      JSON.parse(resp.body)
    end
  end
end
