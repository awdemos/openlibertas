# External Blind Review Session

Session id: ext_20260503_200803_8a067270
Session token: d515dba65c8f9020047f93c248b7348f
Blind packet: /var/home/a/code/openlibertas/.desloppify/review_packet_blind.json
Template output: /var/home/a/code/openlibertas/.desloppify/external_review_sessions/ext_20260503_200803_8a067270/review_result.template.json
Claude launch prompt: /var/home/a/code/openlibertas/.desloppify/external_review_sessions/ext_20260503_200803_8a067270/claude_launch_prompt.md
Expected reviewer output: /var/home/a/code/openlibertas/.desloppify/external_review_sessions/ext_20260503_200803_8a067270/review_result.json

Happy path:
1. Open the Claude launch prompt file and paste it into a context-isolated subagent task.
2. Reviewer writes JSON output to the expected reviewer output path.
3. Submit with the printed --external-submit command.

Reviewer output requirements:
1. Return JSON with top-level keys: session, assessments, issues.
2. session.id must be `ext_20260503_200803_8a067270`.
3. session.token must be `d515dba65c8f9020047f93c248b7348f`.
4. Include issues with required schema fields (dimension/identifier/summary/related_files/evidence/suggestion/confidence).
5. Use the blind packet only (no score targets or prior context).
