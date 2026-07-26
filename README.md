# DNS Conduit

DNS forwarding and observability platform (Rust).

## AI-assisted development

This project was built with extensive assistance from AI tools. Some operators
and contributors prefer software written without that involvement — a view I 
can respect, even if I don't agree with it. I am not currently planning to reevaluate how
DNS Conduit is developed, and I will not engage in arguments about that decision.


## Documentation

Operator documentation (install, configure, operate, troubleshoot):

**https://egon1024.github.io/DNSConduit/**

## Performance harness (lab)

Binary-driven load suites live under [perf/](perf/) (`python3 -m perf.runner`). Replay against a Conduit binary with Docker-pinned dnsperf by default — no rustc required for suite replay. See [perf/README.md](perf/README.md). Microbenchmarks remain `make performance` (distinct from suite runs).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full text.

Contributions require a [Developer Certificate of Origin](https://developercertificate.org/)
sign-off; see [CONTRIBUTING.md](CONTRIBUTING.md).

