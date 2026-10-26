---
name: office-meetings
description: The meeting pack — transcribe a recording, summarise it, split decisions into your to-dos and agents' to-dos, and update the Library with citations.
---

# Meetings (scaffold)

Stub skill. The hero loop lands with the meeting slice:

1. A recording is captured as a `meeting` source.
2. `office.transcribe` produces speaker turns with `[hh:mm:ss]` anchors.
3. `office.extract` produces a summary, decisions and to-dos as structured JSON.
4. To-dos split into yours and agents'; outward-facing drafts wait for approval.
5. Library pages update, each claim citing the meeting anchor it came from.
