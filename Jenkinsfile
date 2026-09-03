// benix-mdns-advertiser CI: fmt -> clippy -> test -> build, plus a real musl
// cross-compile stage. Modeled on the shared kit Jenkinsfile template
// (slash-builder/storage-kit) and benix-compositor's own Jenkinsfile shape,
// simplified: unlike the compositor, this crate has zero native library
// dependencies (pure Rust + libc socket syscalls), so there's no apt-get
// leg of DRM/GBM/libinput/etc headers to install — just musl-tools.
//
// No publish stage yet. README.md's "Known gaps" section names this
// explicitly: qr-gateway/qr-cli went through the same two-step sequence
// (dinit units first against an existing artifact, a musl release target
// added second, PR #44) — this crate is only at the first step's
// equivalent.
//
// musl cross-compile stage runs `apt-get install musl-tools` inside its
// own docker agent. Jenkins' Docker Pipeline plugin runs that container as
// the host agent's own fixed uid (5005) by default, not root — confirmed
// live against this exact stage (PR #3, build #1: `docker run -u 5005:5005
// ... rust:1.90-trixie cat` then `apt-get update` -> "Permission denied"
// on /var/lib/apt/lists/lock). Same root cause substrate-kit's own
// Jenkinsfile already documents and already fixes for its own privileged
// stage (`args '-u root:root'`) — applying the identical, already-
// established fix here rather than a new pattern.
def RUST_IMAGE = 'rust:1.90-trixie'

pipeline {
  agent { label 'linux-build' }

  stages {

    stage('Pre-flight') {
      steps {
        script {
          def msg = sh(script: 'git log -1 --pretty=%B', returnStdout: true).trim()
          if (msg.contains('[skip ci]')) {
            currentBuild.result = 'NOT_BUILT'
            error('Commit contains [skip ci] — aborting.')
          }
        }
      }
    }

    stage('Build & Test') {
      agent { docker { image RUST_IMAGE; reuseNode true } }
      steps {
        sh '''
          rustup component add rustfmt clippy
          cargo fmt --check
          cargo clippy --all-targets -- -D warnings
          cargo test
          cargo build --release
        '''
      }
    }

    stage('musl cross-compile') {
      agent { docker { image RUST_IMAGE; args '-u root:root'; reuseNode true } }
      steps {
        sh '''
          apt-get update -qq
          apt-get install -y -qq --no-install-recommends musl-tools
          rustup target add x86_64-unknown-linux-musl
          cargo build --release --target x86_64-unknown-linux-musl
        '''
      }
    }
  }
  post {
    success { echo 'benix-mdns-advertiser pipeline succeeded' }
    failure { echo 'Pipeline failed.' }
    always  { cleanWs() }
  }
}
