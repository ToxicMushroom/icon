Get a flamegraph and perf data from the test_real_pre_population test:
`RUSTFLAGS="-C target-cpu=native" CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --features cache,log --unit-test -- test_real_pre_population`

Convert perf.data into a perf file
`perf script -F +pid > /tmp/test.perf`

You can inspect /tmp/test.perf on https://profiler.firefox.com/