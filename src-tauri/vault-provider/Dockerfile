# vault-provider: a portable container (spec v0.4 §11.1). Plain HTTP on
# $PROVIDER_PORT; TLS is terminated by the hosting platform in front of
# it. Configure with the environment variables in src/config.rs.
FROM rust:1-bookworm AS build
WORKDIR /src
# The service builds from its own workspace; the two path crates it uses
# sit beside it in the repository.
COPY vault-proto ./vault-proto
COPY vault-provider-core ./vault-provider-core
COPY vault-provider ./vault-provider
RUN cd vault-provider && cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 provider
COPY --from=build /src/vault-provider/target/release/vault-provider /usr/local/bin/vault-provider
USER provider
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/vault-provider"]
CMD ["serve"]
