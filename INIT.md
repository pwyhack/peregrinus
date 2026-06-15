https://instantdomainsearch.com/ <--

I want to re-create this for internet and addresses.

Basically right now to aggregate options, real quotes, pricing, speed's etc for internet to my house its super disparate and really annoying. Would be great to have a super quick, snappy website that can just crank really really hard.

Rough ideas for architecture and tech stack:

will probably use rust most likely, if I had to guess.
https://instantdomainsearch.com/ <-- main inspiration
[https://github.com/instant-labs/](https://github.com/instant-labs/) <-- company code, interesting,

for hosting:
It is **not fully knowable from the outside**, but a lot is public.

The observable / documented stack is roughly:

```text
Frontend:
React
Next.js
TypeScript
Static export where possible
CDN-served HTML/CSS/JS

Backend:
Rust service for domain search
GraphQL / async-graphql appears likely from public engineer resumes
Postgres likely for registrar/account/product data
Custom optimized lookup/indexing systems

Search / ML:
Rust
ONNX Runtime
HNSW approximate nearest neighbor index
fastText historically
sentence-transformers / all-MiniLM-L6-v2 for domain embeddings
GPT-4 used offline to generate training data
SQLite used in prototype mapping layer
~25M aftermarket-domain index

Infra:
Google Cloud Platform
Google Cloud CDN
Caddy in front of Next.js for headers/proxying
BigQuery for analytics / Core Web Vitals logging
Custom analytics instead of heavy third-party scripts
Likely Buildkite / Pulumi in parts of infra, based on public repos
```

The cleanest source is their own Core Web Vitals case study: they say the frontend is **React served with Next.js**, with a **Rust backend service** answering domain searches. They also say most content is statically exported and served from a CDN, while the hydrated app directly queries the Rust backend for fast search results. ([[Smashing Magazine](https://www.smashingmagazine.com/2021/05/core-web-vitals-case-study/)][1])

Their newer AI/search writeup says a lot of the performance-critical stack is Rust. They describe using **fastText + Annoy** historically, then moving to **sentence-transformers**, **GPT-4-generated training data**, **all-MiniLM-L6-v2**, **ONNX Runtime**, **Rust + ort**, **HNSW**, CPU-only inference, and quantization to hit about 10 ms p95 latency. ([[Instant Domain Search](https://instantdomainsearch.com/learn/research/building-fast-ai)][2])

For word segmentation, they explicitly ported Python logic to **Rust**, using crates/techniques like `ahash`, `smartstring`, `Vec`, some unsafe ASCII-path optimization, and a triangular matrix algorithm. ([[Instant Domain Search](https://instantdomainsearch.com/learn/research/instant-word-segmentation-with-rust)][3])

One extra signal: a former engineer’s public resume lists work around **React, TypeScript, GraphQL, Relay, Rust, Postgres**, and specifically mentions Instant Domain Search work involving **Relay GraphQL with a Rust async-graphql API**, a user dashboard in **Next.js/React/Relay**, and a React Native mobile app. ([[timbaker.me](https://timbaker.me/assets/resume.pdf?utm_source=chatgpt.com)][4])

So the short version:

**Instant Domain Search is basically a very optimized React/Next.js app sitting on a Rust search backend, with custom domain-specific ML/indexing infrastructure, CDN-first delivery, and a GCP/BigQuery performance/analytics layer.**

The important architectural lesson is not “they used React.” It is that they moved the latency-sensitive parts out of the normal web-app path: static frontend, direct backend calls, Rust for hot code, precomputed indexes, offline AI, tiny models, ONNX, and ruthless avoidance of runtime bloat.

[1]: https://www.smashingmagazine.com/2021/05/core-web-vitals-case-study/ "How We Improved Our Core Web Vitals (Case Study) — Smashing Magazine"
[2]: https://instantdomainsearch.com/learn/research/building-fast-ai "Building Fast AI for Domain Search | Instant Domain Search"
[3]: https://instantdomainsearch.com/learn/research/instant-word-segmentation-with-rust "Word Segmentation with Rust | Instant Domain Search"
[4]: https://timbaker.me/assets/resume.pdf?utm_source=chatgpt.com "resume.pdf - Tim Baker"

So we shouldn't copy 1:1 for a prototype but we should figure out what we can do with this and see if we can't recreate the rust engine, the website and a super fast aggregator for internet options with any address

Will probably require some finangaling with web infra and stuff but should be fun! Don't stop until you have atleast a version of an aggregator where on a website, on localhost:1313 I can search my address (exact address) and see all the internet providers that _actually_ will serve me, cost, plan options etc

thanks!
use rust, and next.js 16 I guess? shadcn, and copy the design language from instantdomainsearch. We will plan to deploy literally everything onto cloudflare so just use any/all of their resources and we will codename this

"peregrinus" is the codename/repo name!

rust, next.js, cloudflare free plan, ill buy the domain if you can get something cooking.

Find some rust skills and stuff so you write good rust code, the rust compiler is also really good to check your work against
cool, you got this
