import matplotlib.pyplot as plt
import os
import numpy as np

def read_file_get_data(path):
    file = open(path)
    iter_column = []
    time_column = []
    while file.readline() != "":
        content = file.readline().split()
        iter_column.append(int(content[0]))
        time_column.append(int(content[1])/1000)
    return (iter_column, time_column)

iter1, time1 = read_file_get_data('./parallel_evaluation.txt')
iter2, time2 = read_file_get_data('./trta_evaluation.txt')
x1 = np.array(iter1)
y1 = np.array(time1)
y2 = np.array(time2)

# plt.axis('equal')
plt.xlabel("Number of commands")
plt.ylabel("Seconds")
plt.plot(x1, y1)
plt.plot(x1, y2)
plt.xticks(np.arange(0, x1[-1], 25))
plt.yticks(np.arange(0, y2[-1], 2))

plt.savefig('./plot.png')