Feature: Skill bundle file reads

  Scenario: A model reads a referenced bundled text file
    Given a loaded skill bundle with a referenced usage file
    When the model calls skill_read_file for the usage file
    Then the tool returns the referenced text without a host filesystem path

  Scenario: A model is denied raw filesystem traversal
    Given a loaded skill bundle with a referenced usage file
    When the model calls skill_read_file with a traversal path
    Then the tool returns a skill-scoped path_not_readable error
