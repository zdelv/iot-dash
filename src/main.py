import argparse

def test(a: int) -> None:
    print(a)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--test')
