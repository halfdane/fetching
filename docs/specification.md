# Project Specification: fetching (Architecture & Testability)

In the end, I'd like to have a sexy, yet functional frontend that helps users understand what's going on without overwhelming them with minute details.
It's not an expert tool, but experts might value being able to see more details if they wish.

## Logical Compartmentalization

- **Authentication & Session Management**
	- Handles OAuth flow, token storage, and automatic refresh.
	- Abstracts session creation and renewal, ensuring seamless long-running operation.

- **Configuration Management**
	- Centralizes all runtime settings (OAuth credentials, directories, timeouts, etc.).
	- Supports environment variable overrides and provides sensible defaults.

- **Streaming & Caching Logic**
	- Orchestrates the retrieval and local storage of tracks, albums, and playlists.
	- Manages audio streaming, file writing, and metadata tagging.
	- Handles cover art download and embedding.

- **Metadata Handling**
	- Extracts, sanitizes, and applies metadata for tracks and albums.
	- Ensures consistent file naming and tagging for offline playback.

- **Audio Playback**
	- Plays locally cached audio files through the system’s audio output.
	- Supports sequential playback and error handling.

- **Playlist Generation**
	- Generates M3U8 playlists from cached content, including metadata and optional Spotify URLs.

- **Error Handling**
	- Uses structured error types for all major failure modes (network, I/O, authentication, etc.).
	- Centralizes error reporting and recovery strategies.

## Architectural Patterns

- **Separation of Concerns**
	- Each logical area (auth, config, streaming, metadata, playback) is isolated, minimizing cross-dependencies.

- **Trait-Based Abstraction**
	- Core operations (e.g., audio downloading and generally librespot dependencies) are defined as traits, enabling dependency injection and mocking.
	- Facilitates testability and future extensibility (e.g., swapping out streaming backends).

- **Asynchronous Programming**
	- Uses async/await for all I/O-bound operations (network, file, streaming), ensuring scalability and responsiveness.

- **Background Task Management**
	- Long-running operations (e.g., token refresh) are handled by background tasks, decoupled from main logic.

- **Configuration via Environment**
	- All runtime settings are externally configurable, supporting both development and production use cases.

- **Error Propagation**
	- Uses rich error types and propagation to surface actionable diagnostics at all layers.

## Test Approaches

- **Trait-Based Mocking**
	- All external dependencies (network, Spotify API, file I/O) are abstracted behind traits, allowing for in-memory or mock implementations in tests.

- **Unit Testing**
	- Pure logic (e.g., metadata sanitization, error classification) is covered by fast, isolated unit tests.

- **Integration Testing**
	- End-to-end scenarios (e.g., playlist generation, error recovery, cover collage creation) are tested with real or mock data.

- **Error Simulation**
	- Mocks can simulate network failures, authentication errors, and retriable conditions to verify robustness.

- **Test Data Management**
	- Uses sample URIs and metadata for repeatable, deterministic tests.

- **Continuous Testability**
	- Architecture is designed to allow new features to be tested in isolation by extending trait abstractions and adding new test cases.

## Summary

The architecture emphasizes modularity, testability, and resilience. By compartmentalizing logic, abstracting dependencies, and prioritizing asynchronous, error-aware design, the system is robust, maintainable, and easy to extend or test in isolation.
