# A certificate identifies an agent, a token authorizes participation at a resource

smbdrop's event channel tells connections apart by their client certificate, not by their login
token: anonymous agents log in with a token borrowed from a certified agent at their resource, so
the token's `sub` names the lender, not the connection. The token authorizes the connection to
take part at its resource. The certificate, when present, decides who it is. smbdrop tracks connections,
each with its own ID, presence and mount status, not identities.

Agents say which kind they are with the login message, and smbdrop checks that against the TLS
connection:

| Login message    | Certificate                              | The connection is                                     |
| ---------------- | ---------------------------------------- | ----------------------------------------------------- |
| `Login`          | present, AgentId equals the token's `sub` | that certified agent, shown as verified               |
| `Login`          | present, AgentId differs                 | rejected                                              |
| `Login`          | none                                     | rejected                                              |
| `AnonymousLogin` | none                                     | an anonymous participant, under the name it reports   |
| `AnonymousLogin` | present                                  | rejected                                              |

Anonymous agents are full participants at a resource: they run where a client certificate
can't be stored safely, for example on machines where user software runs with elevated
privileges, or on machines that aren't permanent members of a resource.
Their name is only for display and is not a trusted identity.

## Considered options

- **Key by the token's AgentId and count connections per id.** Fixes removal on disconnect, but
  mount status stays last-writer-wins and an anonymous connection keeps the lender listed after
  it left.
- **A delegation claim in tokens handed out by a certified agent's token proxy.** Also
  unforgeable, but needs a token-service change, while the certificate is already on every
  certified agent's event-channel connection.
- **Trust what the login says.** A self-declared "anonymous" flag, unchecked: a client that
  leaves it out would still be filed under the lender. The login variant is a claim too, which
  is why smbdrop checks it against the certificate.
- **One `Login` with an optional `name`.** A name from a certified agent would be one more field
  that can contradict the certificate; separate variants carry only what each kind of agent has.

## Consequences

- A certified agent never borrows a token: it fetches its own from the token service over mTLS,
  which signs the certificate's AgentId. A rejection can only come from a bug or a
  misconfiguration, so it is logged as an error on both sides instead of being worked around.
- A device that moves between resources is configured as anonymous and carries no certificate.
- The same certified agent may briefly appear twice, for example while smbdrop still holds a
  half-dead connection after a reconnect. This is accepted.
