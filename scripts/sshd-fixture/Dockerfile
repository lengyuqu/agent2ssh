# Throwaway sshd used only by src-tauri/tests/exec_fixture.rs.
#
# Password auth is enabled with a fixed, well-known credential so the suite can
# connect without keys, and it listens inside the container only. Never run this
# anywhere reachable from an untrusted network.
FROM alpine:3.20
RUN apk add --no-cache openssh && \
    ssh-keygen -A && \
    echo 'root:a2s-fixture' | chpasswd && \
    sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config && \
    sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config && \
    sed -i 's/^#\?UsePAM.*/UsePAM no/' /etc/ssh/sshd_config
EXPOSE 22
CMD ["/usr/sbin/sshd","-D","-e","-o","LogLevel=ERROR"]
