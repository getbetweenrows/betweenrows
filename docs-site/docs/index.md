---
title: BetweenRows docs
description: Documentation for BetweenRows — a fully customizable data access governance layer. Install, configure, and operate the SQL-aware proxy that enforces fine-grained access policies across your databases, warehouses, and lakehouses.
layout: home

hero:
  name: BetweenRows
  text: Documentation
  tagline: A fully customizable data access governance layer — install, configure, and operate the SQL-aware proxy that enforces fine-grained access policies in real-time.
  image:
    src: /logo.svg
    alt: BetweenRows
  actions:
    - theme: brand
      text: Quickstart
      link: /start/quickstart
    - theme: alt
      text: Introduction
      link: /start/introduction
    - theme: alt
      text: betweenrows.dev
      link: https://betweenrows.dev

features:
  - title: Start
    details: Introduction, Quickstart, and the demo schema. From a single `docker run` command to a policy-protected query in under 15 minutes.
    link: /start/quickstart
    linkText: Get started
  - title: Concepts
    details: Architecture, policy model, security overview, threat model, known limitations, and glossary. Begin here if you are evaluating BetweenRows.
    link: /concepts/architecture
    linkText: Understand the system
  - title: Features
    details: Data sources, users and roles, attributes, the policy system (types, template expressions, decision functions), and audit and debugging.
    link: /guides/data-sources
    linkText: Explore features
  - title: Guides
    details: Deploy with Docker or Fly, configure, upgrade, back up, troubleshoot, and follow end-to-end recipes like multi-tenant isolation.
    link: /installation/docker
    linkText: Run in production
  - title: About
    details: Roadmap, changelog, and license.
    link: /about/roadmap
    linkText: See the roadmap
---

::: tip
Pin your Docker image to a specific version tag (e.g., `ghcr.io/getbetweenrows/betweenrows:{{VERSION}}`) rather than `:latest` so upgrades stay deliberate.
:::

## For security and compliance reviewers

Start with the **[Security Overview](/concepts/security-overview)** — a curation page that frames the threat model, trust boundaries, and deployment checklist, then links to the detailed concept and reference pages. Then skim **[Architecture](/concepts/architecture)**, **[Policy Model](/concepts/policy-model)**, and **[Known Limitations](/operations/known-limitations)**.

## For developers and DBAs

Start with the **[Quickstart](/start/quickstart)** — it walks from a single `docker run` command to a working policy-protected query in under 15 minutes. Then read the **[Write your first row filter](/guides/policies/row-filters)** guide and the **[Multi-tenant isolation](/guides/recipes/multi-tenant-isolation)** flagship tutorial.
