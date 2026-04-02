# frozen_string_literal: true

module HeraldAdmin
  class HeraldError < StandardError
    attr_reader :code, :status

    def initialize(code, message, status: 0)
      super("#{code}: #{message}")
      @code = code
      @status = status
    end
  end
end
