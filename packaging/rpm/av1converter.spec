%global debug_package %{nil}

Name: av1converter
Version: %{av1converter_version}
Release: 1%{?dist}
Summary: Batch-convert video files to AV1 using FFmpeg
License: MIT
URL: https://gitlab.com/francescomicheli/av1converter
Source0: av1converter
Source1: LICENSE
Requires: ffmpeg

%description
AV1Converter provides an interactive terminal UI and a local web UI for
managing FFmpeg encoding, track selection, and quality verification.

%install
install -Dpm 0755 %{SOURCE0} %{buildroot}%{_bindir}/av1converter
install -Dpm 0644 %{SOURCE1} %{buildroot}%{_licensedir}/%{name}/LICENSE

%files
%{_bindir}/av1converter
%license %{_licensedir}/%{name}/LICENSE
