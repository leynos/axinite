Feature: Active skill bundle metadata

  Scenario: Selected bundle skill exposes stable bundle-relative metadata
    Given an installed bundled skill with supporting files
    When the skill is selected for an agent turn
    Then the active skill context names the skill identifier
    And the active skill context names SKILL.md as the entrypoint
    And the active skill context does not expose the filesystem root
