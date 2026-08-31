package tqlmate

final case class TypeDbUrl(
  username: String,
  password: String,
  host: String,
  port: Int,
  database: String,
  tls: Boolean
):
  def address: String = s"$host:$port"

object TypeDbUrl:
  def parse(raw: String): Result[TypeDbUrl] =
    if !raw.startsWith("typedb://") then
      Left(Error.msg("URL must start with typedb://"))
    else
      val rest = raw.substring("typedb://".length)
      rest.split("@", 2) match
        case Array(auth, afterAuth) =>
          auth.split(":", 2) match
            case Array(user, pass) =>
              parseHostDb(afterAuth).map: (host, port, database, tls) =>
                TypeDbUrl(decode(user), decode(pass), host, port, database, tls)
            case _ => Left(Error.msg("URL auth must be user:pass"))
        case _ => Left(Error.msg("URL missing user:pass@host"))

  private def parseHostDb(afterAuth: String): Result[(String, Int, String, Boolean)] =
    val (hostPort, pathQuery) = afterAuth.split("/", 2) match
      case Array(hp, pq) => (hp, pq)
      case Array(hp)     => (hp, "")
    val (path, query) = pathQuery.split("\\?", 2) match
      case Array(p, q) => (p, q)
      case Array(p)    => (p, "")
    val hostPortParsed: Result[(String, Int)] =
      hostPort.lastIndexOf(':') match
        case -1 => Right((hostPort, 1729))
        case i =>
          val h = hostPort.substring(0, i)
          val p = hostPort.substring(i + 1)
          p.toIntOption match
            case Some(n) => Right((h, n))
            case None    => Left(Error.msg(s"invalid port: $p"))
    hostPortParsed.flatMap: (host, port) =>
      val database = path.stripPrefix("/").takeWhile(_ != '/').trim
      if database.isEmpty then Left(Error.msg("URL must include /database"))
      else
        val tls = query
          .split("&")
          .filter(_.nonEmpty)
          .exists: pair =>
            val (k, v) = pair.split("=", 2) match
              case Array(a, b) => (a, b)
              case Array(a)    => (a, "")
            k == "tls" && Set("true", "1", "yes")(v)
        Right((host, port, database, tls))

  private def decode(s: String): String =
    val out = StringBuilder(s.length)
    var i = 0
    while i < s.length do
      s.charAt(i) match
        case '%' if i + 2 < s.length && isHex(s.charAt(i + 1)) && isHex(s.charAt(i + 2)) =>
          out.append(Integer.parseInt(s.substring(i + 1, i + 3), 16).toChar)
          i += 3
        case '+' =>
          out.append(' ')
          i += 1
        case c =>
          out.append(c)
          i += 1
    out.toString

  private def isHex(c: Char): Boolean =
    (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')
