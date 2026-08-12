class Av1converter < Formula
  desc "Batch-convert video files to AV1 using FFmpeg"
  homepage "https://gitlab.com/francescomicheli/av1converter"
  url "https://gitlab.com/francescomicheli/av1converter.git",
      tag:      "v3.0.0",
      revision: "99ea137827ce660abb2c885b9f7174fd896804a5"
  version "3.0.0"
  license "MIT"

  depends_on "rust" => :build
  depends_on "ffmpeg"

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "av1converter #{version}", shell_output("#{bin}/av1converter --version")
  end
end
