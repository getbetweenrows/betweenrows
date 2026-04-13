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
      text: betweenrows.com
      link: https://getbetweenrows.com

features:
  - title: Start
    details: Introduction and Quickstart. From a single `docker run` command to a policy-protected query in under 15 minutes.
    link: /start/quickstart
    linkText: Get started
  - title: How It Works
    details: Architecture, policy model, security overview, and threat model. Begin here if you are evaluating BetweenRows.
    link: /concepts/architecture
    linkText: Read the concepts
  - title: Guides
    details: Data sources, users and roles, attributes, policy authoring, decision functions, audit and debugging, plus end-to-end recipes.
    link: /guides/data-sources
    linkText: Browse guides
  - title: Reference
    details: Configuration, policy types, template expressions, audit log fields, demo schema, CLI, and the Admin REST API.
    link: /reference/configuration
    linkText: Open reference
  - title: Operate
    details: Docker and Fly deployment, upgrading, backups and recovery, rename safety, troubleshooting, and known limitations.
    link: /installation/docker
    linkText: Run in production
  - title: About
    details: Changelog, roadmap, license, and alpha status.
    link: /about/changelog
    linkText: See what is new
---

::: tip
Pin your Docker image to a specific version tag (e.g., `ghcr.io/getbetweenrows/betweenrows:0.15.0`) rather than `:latest` so upgrades stay deliberate. See [License & Alpha Status](/about/license) for the full release-stability picture.
:::

## For security and compliance reviewers

Start with the **[Security Overview](/concepts/security-overview)** — a curation page that frames the threat model, trust boundaries, and deployment checklist, then links to the detailed concept and reference pages. Then skim **[Architecture](/concepts/architecture)**, **[Policy Model](/concepts/policy-model)**, and **[Known Limitations](/operations/known-limitations)**.

## For developers and DBAs

Start with the **[Quickstart](/start/quickstart)** — it walks from a single `docker run` command to a working policy-protected query in under 15 minutes. Then read the **[Write your first row filter](/guides/policies/row-filters)** guide and the **[Multi-tenant isolation](/guides/recipes/multi-tenant-isolation)** flagship tutorial.
