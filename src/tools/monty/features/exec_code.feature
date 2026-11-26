Feature: Exec code callback mediation

  Scenario: A selected tool callback runs once
    Given a caller that authorizes the echo tool
    When the guest calls the selected echo tool
    Then the echo tool has run once
    And the guest receives the tool's JSON result

  Scenario: An unselected tool callback is refused
    Given a caller that authorizes only the echo tool
    When the guest calls the unselected write tool
    Then the call is refused as not_allowed
    And no tool has run

  Scenario: A tool that always requires approval is refused before dispatch
    Given a caller that authorizes a tool requiring approval
    When the guest calls the selected guarded tool
    Then the call is refused as approval_required
    And no tool has run

  Scenario: Recursive code execution is refused
    Given a caller that authorizes the echo tool
    When the guest calls exec_code
    Then the call is refused as recursion_denied
    And no tool has run

  Scenario: A hook rewrite is revalidated before dispatch
    Given a caller whose hook rewrites the arguments out of schema
    When the guest calls the selected echo tool
    Then the call is refused as contract_error
    And no tool has run