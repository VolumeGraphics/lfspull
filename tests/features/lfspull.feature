Feature: A simple git-lfs implementation for pulling single files

  Scenario: Pulling a single file the first time
    Given the test-repository is correctly setup
    When pulling the file
    Then the file was pulled from origin
    And the file size is 226848

  Scenario: Pulling a single file when already cached
    Given the test-repository is correctly setup
    And the file was pulled already
    When resetting the file
    And pulling the file
    Then the file was pulled from local cache
    And the file size is 226848

  Scenario: Pulling a file that already exists
    Given the test-repository is correctly setup
    And the file was pulled already
    When pulling the file
    Then the file was already there
    And the file size is 226848

  Scenario: Pulling a complete directory
    Given the test-repository is correctly setup
    When pulling the complete directory
    Then the file size is 226848
