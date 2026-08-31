package tqlmate

enum Error:
  case Msg(text: String)
  case Io(cause: Throwable)
  case TypeDb(cause: Throwable)

  def message: String = this match
    case Msg(t)    => t
    case Io(c)     => Option(c.getMessage).getOrElse(c.toString)
    case TypeDb(c) => Option(c.getMessage).getOrElse(c.toString)

object Error:
  def msg(m: String): Error = Msg(m)

  def fromThrowable(t: Throwable): Error =
    t match
      case e: java.io.IOException =>
        Io(e)
      case e: com.typedb.driver.common.exception.TypeDBDriverException =>
        TypeDb(e)
      case other =>
        Msg(Option(other.getMessage).getOrElse(other.toString))

type Result[A] = Either[Error, A]
