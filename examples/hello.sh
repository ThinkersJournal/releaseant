# Run inside your git repository (see the README for what each step prints).
SHA=$(git rev-parse HEAD)
releaseant plan --sha $SHA
releaseant publish --sha $SHA        # a dry run: nothing is uploaded
